// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::error::Error;
use std::io;
use std::path::{Path, PathBuf};

use derive_more::Display;
use libfxrecord::error::ErrorExt;
use libfxrecord::net::*;
use libfxrecord::prefs::write_prefs;
use slog::{error, info, Logger};
use tokio::fs::{File, OpenOptions};
use tokio::net::TcpStream;
use tokio::prelude::*;
use tokio::task::spawn_blocking;

use crate::osapi::ShutdownProvider;
use crate::taskcluster::{Taskcluster, TaskclusterError};
use crate::zip::{unzip, ZipError};

/// The runner side of the protocol.
pub struct RunnerProto<S> {
    inner: Option<Proto<RecorderMessage, RunnerMessage, RecorderMessageKind, RunnerMessageKind>>,
    log: Logger,
    shutdown_handler: S,
    tc: Taskcluster,
}

impl<S> RunnerProto<S>
where
    S: ShutdownProvider,
{
    pub fn new(log: Logger, stream: TcpStream, shutdown_handler: S, tc: Taskcluster) -> Self {
        Self {
            inner: Some(Proto::new(stream)),
            log,
            shutdown_handler,
            tc,
        }
    }

    /// Consume the RunnerProto and return the underlying `Proto`.
    pub fn into_inner(
        self,
    ) -> Proto<RecorderMessage, RunnerMessage, RecorderMessageKind, RunnerMessageKind> {
        self.inner.unwrap()
    }

    /// Send the given message to the runner.
    ///
    /// If the underlying proto is None, this will panic.
    async fn send<M>(&mut self, m: M) -> Result<(), ProtoError<RecorderMessageKind>>
    where
        for<'de> M: MessageContent<'de, RunnerMessage, RunnerMessageKind>,
    {
        self.inner.as_mut().unwrap().send(m).await
    }

    /// Receive a given kind of message from the runner.
    ///
    /// If the underlying proto is None, this will panic.
    async fn recv<M>(&mut self) -> Result<M, ProtoError<RecorderMessageKind>>
    where
        for<'de> M: MessageContent<'de, RecorderMessage, RecorderMessageKind>,
    {
        self.inner.as_mut().unwrap().recv::<M>().await
    }

    /// Handshake with FxRecorder.
    pub async fn handshake_reply(&mut self) -> Result<bool, RunnerProtoError<S>> {
        info!(self.log, "Handshaking ...");
        let Handshake { restart } = self.recv().await?;

        if restart {
            if let Err(e) = self
                .shutdown_handler
                .initiate_restart("fxrecord: recorder requested restart")
            {
                error!(self.log, "an error occurred while handshaking"; "error" => ?e);
                self.send(HandshakeReply {
                    result: Err(e.into_error_message()),
                })
                .await?;

                return Err(RunnerProtoError::Shutdown(e));
            }
            info!(self.log, "Restart requested; restarting ...");
        }

        self.send(HandshakeReply { result: Ok(()) }).await?;
        info!(self.log, "Handshake complete");

        Ok(restart)
    }

    pub async fn download_build_reply(
        &mut self,
        download_dir: &Path,
    ) -> Result<PathBuf, RunnerProtoError<S>> {
        let DownloadBuild { task_id } = self.recv().await?;

        info!(self.log, "Received build download request"; "task_id" => &task_id);

        self.send(DownloadBuildReply {
            result: Ok(DownloadStatus::Downloading),
        })
        .await?;

        match self
            .tc
            .download_build_artifact(&task_id, download_dir)
            .await
        {
            Ok(download_path) => {
                self.send(DownloadBuildReply {
                    result: Ok(DownloadStatus::Downloaded),
                })
                .await?;

                let unzip_result = spawn_blocking({
                    let download_dir = PathBuf::from(download_dir);
                    move || unzip(&download_path, &download_dir)
                })
                .await
                .expect("unzip task was cancelled or panicked");

                if let Err(e) = unzip_result {
                    self.send(DownloadBuildReply {
                        result: Err(e.into_error_message()),
                    })
                    .await?;

                    Err(e.into())
                } else {
                    let firefox_path = download_dir.join("firefox").join("firefox.exe");

                    if !firefox_path.exists() {
                        let err = RunnerProtoError::MissingFirefox;
                        self.send(DownloadBuildReply {
                            result: Err(err.into_error_message()),
                        })
                        .await?;

                        Err(err)
                    } else {
                        self.send(DownloadBuildReply {
                            result: Ok(DownloadStatus::Extracted),
                        })
                        .await?;

                        Ok(firefox_path)
                    }
                }
            }

            Err(e) => {
                error!(self.log, "could not download build"; "error" => ?e);
                self.send(DownloadBuildReply {
                    result: Err(e.into_error_message()),
                })
                .await?;
                Err(e.into())
            }
        }
    }

    pub async fn send_profile_reply(
        &mut self,
        download_dir: &Path,
    ) -> Result<Option<PathBuf>, RunnerProtoError<S>> {
        info!(self.log, "Waiting for profile...");

        let SendProfile { profile_size } = self.recv().await?;

        let profile_size = match profile_size {
            Some(profile_size) => profile_size,
            None => {
                info!(self.log, "No profile provided");
                self.send(SendProfileReply { result: Ok(None) }).await?;

                return Ok(None);
            }
        };

        info!(self.log, "Receiving profile...");
        self.send(SendProfileReply {
            result: Ok(Some(DownloadStatus::Downloading)),
        })
        .await?;

        let mut stream = self.inner.take().unwrap().into_inner();
        let result = Self::send_profile_reply_impl(
            &mut stream,
            download_dir,
            profile_size,
        )
        .await;
        self.inner = Some(Proto::new(stream));

        info!(self.log, "Profile received; extracting...");

        let zip_path = match result {
            Ok(zip_path) => {
                self.send(SendProfileReply {
                    result: { Ok(Some(DownloadStatus::Downloaded)) },
                })
                .await?;
                zip_path
            }

            Err(e) => {
                self.send(SendProfileReply {
                    result: { Err(e.into_error_message()) },
                })
                .await?;
                return Err(e);
            }
        };

        let unzip_path = download_dir.join("profile");

        let unzip_result = spawn_blocking({
            let zip_path = zip_path.clone();
            let unzip_path = unzip_path.clone();
            move || unzip(&zip_path, &unzip_path)
        })
        .await
        .expect("unzip profile task was cancelled or panicked");

        let stats = match unzip_result {
            Ok(stats) => stats,
            Err(e) => {
                error!(self.log, "Could not extract profile"; "error" => ?e);

                self.send(SendProfileReply {
                    result: Err(e.into_error_message()),
                })
                .await?;

                return Err(e.into());
            }
        };

        if stats.extracted == 0 {
            error!(self.log, "Profile was empty!");
            let e = RunnerProtoError::EmptyProfile;
            self.send(SendProfileReply {
                result: Err(e.into_error_message()),
            })
            .await?;

            return Err(e);
        }

        error!(self.log, "Profile extracted");

        let profile_dir = match stats.top_level_dir {
            Some(top_level_dir) => unzip_path.join(top_level_dir),
            None => unzip_path,
        };

        self.send(SendProfileReply {
            result: { Ok(Some(DownloadStatus::Extracted)) },
        })
        .await?;

        Ok(Some(profile_dir))
    }

    async fn send_profile_reply_impl(
        stream: &mut TcpStream,
        download_dir: &Path,
        profile_size: u64,
    ) -> Result<PathBuf, RunnerProtoError<S>> {
        let zip_path = download_dir.join("profile.zip");
        let mut f = File::create(&zip_path).await?;

        tokio::io::copy(&mut stream.take(profile_size), &mut f).await?;

        Ok(zip_path)
    }

    pub async fn send_prefs_reply(&mut self, prefs_path: &Path) -> Result<(), RunnerProtoError<S>> {
        let SendPrefs { prefs } = self.recv().await?;

        if prefs.is_empty() {
            return self
                .send(SendPrefsReply { result: Ok(()) })
                .await
                .map_err(Into::into);
        }

        let mut f = match OpenOptions::new()
            .append(true)
            .create(true)
            .open(&prefs_path)
            .await
        {
            Ok(f) => f,
            Err(e) => {
                self.send(SendPrefsReply {
                    result: Err(e.into_error_message()),
                })
                .await?;
                return Err(e.into());
            }
        };

        match write_prefs(&mut f, prefs.into_iter()).await {
            Ok(()) => {
                self.send(SendPrefsReply { result: Ok(()) }).await?;
                Ok(())
            }
            Err(e) => {
                self.send(SendPrefsReply {
                    result: Err(e.into_error_message()),
                })
                .await?;
                Err(e.into())
            }
        }
    }
}

#[derive(Debug, Display)]
pub enum RunnerProtoError<S: ShutdownProvider> {
    Proto(ProtoError<RecorderMessageKind>),

    Shutdown(S::Error),

    Taskcluster(TaskclusterError),

    #[display(fmt = "No firefox.exe in build artifact")]
    MissingFirefox,

    #[display(fmt = "An empty profile was received")]
    EmptyProfile,

    Zip(ZipError),
}

impl<S> Error for RunnerProtoError<S>
where
    S: ShutdownProvider,
{
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            RunnerProtoError::Proto(ref e) => Some(e),
            RunnerProtoError::Shutdown(ref e) => Some(e),
            RunnerProtoError::Taskcluster(ref e) => Some(e),
            RunnerProtoError::Zip(ref e) => Some(e),
            RunnerProtoError::MissingFirefox => None,
            RunnerProtoError::EmptyProfile => None,
        }
    }
}

impl<S> From<ProtoError<RecorderMessageKind>> for RunnerProtoError<S>
where
    S: ShutdownProvider,
{
    fn from(e: ProtoError<RecorderMessageKind>) -> Self {
        RunnerProtoError::Proto(e)
    }
}

impl<S> From<TaskclusterError> for RunnerProtoError<S>
where
    S: ShutdownProvider,
{
    fn from(e: TaskclusterError) -> Self {
        RunnerProtoError::Taskcluster(e)
    }
}
impl<S> From<ZipError> for RunnerProtoError<S>
where
    S: ShutdownProvider,
{
    fn from(e: ZipError) -> Self {
        RunnerProtoError::Zip(e)
    }
}

impl<S> From<io::Error> for RunnerProtoError<S>
where
    S: ShutdownProvider,
{
    fn from(e: io::Error) -> Self {
        RunnerProtoError::Proto(ProtoError::Io(e))
    }
}
