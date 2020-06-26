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
use tempfile::TempDir;
use tokio::fs::{create_dir_all, File, OpenOptions};
use tokio::net::TcpStream;
use tokio::prelude::*;
use tokio::task::spawn_blocking;

use crate::osapi::{cpu_and_disk_idle, PerfProvider, ShutdownProvider, WaitForIdleError};
use crate::taskcluster::{Taskcluster, TaskclusterError};
use crate::zip::{unzip, ZipError};

/// The runner side of the protocol.
pub struct RunnerProto<S, P> {
    inner: Option<Proto<RecorderMessage, RunnerMessage, RecorderMessageKind, RunnerMessageKind>>,
    log: Logger,
    shutdown_handler: S,
    tc: Taskcluster,
    perf_provider: P,
}

impl<S, P> RunnerProto<S, P>
where
    S: ShutdownProvider,
    P: PerfProvider + 'static,
{
    /// Handle a request from the recorder.
    pub async fn handle_request(
        log: Logger,
        stream: TcpStream,
        shutdown_handler: S,
        tc: Taskcluster,
        perf_provider: P,
    ) -> Result<bool, RunnerProtoError<S, P>> {
        let mut proto = Self {
            inner: Some(Proto::new(stream)),
            log,
            shutdown_handler,
            tc,
            perf_provider,
        };

        match proto.recv::<Request>().await?.request {
            RecorderRequest::NewRequest(req) => {
                proto.handle_new_request(req).await?;
                Ok(true)
            }

            RecorderRequest::ResumeRequest(req) => {
                proto.handle_resume_request(req).await?;
                Ok(false)
            }
        }
    }
    /// Handle a new request from the recorder.
    async fn handle_new_request(
        &mut self,
        request: NewRequest,
    ) -> Result<(), RunnerProtoError<S, P>> {
        let download_dir = TempDir::new()?;

        let firefox_bin = self
            .download_build(&request.build_task_id, download_dir.path())
            .await?;
        assert!(firefox_bin.is_file());

        let profile_path = match request.profile_size {
            Some(profile_size) => self.recv_profile(profile_size, download_dir.path()).await?,
            None => {
                let profile_path = download_dir.path().join("profile");
                info!(self.log, "Creating new empty profile");
                create_dir_all(&profile_path).await?;
                profile_path
            }
        };
        assert!(profile_path.is_dir());

        if request.prefs.len() > 0 {
            let prefs_path = profile_path.join("user.js");
            let mut f = match OpenOptions::new()
                .append(true)
                .create(true)
                .open(&prefs_path)
                .await
            {
                Ok(f) => f,
                Err(e) => {
                    self.send(WritePrefs {
                        result: Err(e.into_error_message()),
                    })
                    .await?;

                    return Err(e.into());
                }
            };

            if let Err(e) = write_prefs(&mut f, request.prefs.into_iter()).await {
                self.send(WritePrefs {
                    result: Err(e.into_error_message()),
                })
                .await?;
                return Err(e.into());
            }
        }

        self.send(WritePrefs { result: Ok(()) }).await?;

        // TODO: Persist the profile and Firefox instance for a restart

        if let Err(e) = self
            .shutdown_handler
            .initiate_restart("fxrunner: restarting for cold Firefox start")
        {
            // TODO: Once we persist firefox and profile, we need
            error!(self.log, "Could not restart"; "error" => ?e);
            self.send(Restarting {
                result: Err(e.into_error_message()),
            })
            .await?;

            return Err(RunnerProtoError::Shutdown(e));
        }

        self.send(Restarting { result: Ok(()) }).await?;

        Ok(())
    }

    /// Handle a resume request from the runner.
    async fn handle_resume_request(
        &mut self,
        _request: ResumeRequest,
    ) -> Result<(), RunnerProtoError<S, P>> {
        info!(self.log, "Received resumption request");

        self.send(ResumeResponse { result: Ok(()) }).await?;

        info!(self.log, "Waiting to become idle");

        if let Err(e) = cpu_and_disk_idle(&self.perf_provider).await {
            error!(self.log, "CPU and disk did not become idle"; "error" => %e);
            self.send(WaitForIdle {
                result: Err(e.into_error_message()),
            })
            .await?;

            return Err(RunnerProtoError::WaitForIdle(e));
        }
        info!(self.log, "Became idle");

        self.send(WaitForIdle { result: Ok(()) }).await?;

        Ok(())
    }

    /// Download a build from taskcluster.
    async fn download_build(
        &mut self,
        task_id: &str,
        download_dir: &Path,
    ) -> Result<PathBuf, RunnerProtoError<S, P>> {
        info!(self.log, "Download build from Taskcluster"; "task_id" => &task_id);
        self.send(DownloadBuild {
            result: Ok(DownloadStatus::Downloading),
        })
        .await?;

        let download_path = match self.tc.download_build_artifact(task_id, download_dir).await {
            Ok(download_path) => download_path,
            Err(e) => {
                error!(self.log, "Could not download build"; "error" => ?e);
                self.send(DownloadBuild {
                    result: Err(e.into_error_message()),
                })
                .await?;
                return Err(e.into());
            }
        };

        self.send(DownloadBuild {
            result: Ok(DownloadStatus::Downloaded),
        })
        .await?;
        info!(self.log, "Extracting downloaded artifact...");

        let unzip_result = spawn_blocking({
            let download_dir = PathBuf::from(download_dir);
            move || unzip(&download_path, &download_dir)
        })
        .await
        .expect("unzip task was cancelled or panicked");

        if let Err(e) = unzip_result {
            self.send(DownloadBuild {
                result: Err(e.into_error_message()),
            })
            .await?;
            return Err(e.into());
        }

        let firefox_path = download_dir.join("firefox").join("firefox.exe");
        if !firefox_path.exists() {
            let err = RunnerProtoError::MissingFirefox;

            self.send(DownloadBuild {
                result: Err(err.into_error_message()),
            })
            .await?;

            return Err(err);
        }

        info!(self.log, "Extracted build");
        self.send(DownloadBuild {
            result: Ok(DownloadStatus::Extracted),
        })
        .await?;
        Ok(firefox_path)
    }

    /// Receive a profile from the recorder.
    async fn recv_profile(
        &mut self,
        profile_size: u64,
        download_dir: &Path,
    ) -> Result<PathBuf, RunnerProtoError<S, P>> {
        info!(self.log, "Receiving profile...");
        self.send(RecvProfile {
            result: Ok(DownloadStatus::Downloading),
        })
        .await?;

        let mut stream = self.inner.take().unwrap().into_inner();
        let result = Self::recv_profile_raw(&mut stream, download_dir, profile_size).await;
        self.inner = Some(Proto::new(stream));

        let zip_path = match result {
            Ok(zip_path) => zip_path,
            Err(e) => {
                self.send(DownloadBuild {
                    result: Err(e.into_error_message()),
                })
                .await?;
                return Err(e.into());
            }
        };

        info!(self.log, "Profile received; extracting...");
        self.send(RecvProfile {
            result: Ok(DownloadStatus::Downloaded),
        })
        .await?;

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

                self.send(RecvProfile {
                    result: Err(e.into_error_message()),
                })
                .await?;

                return Err(e.into());
            }
        };

        if stats.extracted == 0 {
            error!(self.log, "Profile was empty");
            let e = RunnerProtoError::EmptyProfile;
            self.send(RecvProfile {
                result: Err(e.into_error_message()),
            })
            .await?;

            return Err(e);
        }

        info!(self.log, "Profile extracted");

        let profile_dir = match stats.top_level_dir {
            Some(top_level_dir) => unzip_path.join(top_level_dir),
            None => unzip_path,
        };

        self.send(RecvProfile {
            result: { Ok(DownloadStatus::Extracted) },
        })
        .await?;

        Ok(profile_dir)
    }

    /// Receive the raw bytes of a profile from the recorder.
    async fn recv_profile_raw(
        stream: &mut TcpStream,
        download_dir: &Path,
        profile_size: u64,
    ) -> Result<PathBuf, RunnerProtoError<S, P>> {
        let zip_path = download_dir.join("profile.zip");
        let mut f = File::create(&zip_path).await?;

        tokio::io::copy(&mut stream.take(profile_size), &mut f).await?;

        Ok(zip_path)
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
}

#[derive(Debug, Display)]
pub enum RunnerProtoError<S, P>
where
    S: ShutdownProvider,
    P: PerfProvider + 'static,
{
    #[display(fmt = "An empty profile was received")]
    EmptyProfile,

    #[display(fmt = "No firefox.exe in build artifact")]
    MissingFirefox,

    Proto(ProtoError<RecorderMessageKind>),

    Shutdown(S::Error),

    Taskcluster(TaskclusterError),

    WaitForIdle(WaitForIdleError<P>),

    Zip(ZipError),
}

impl<S, P> Error for RunnerProtoError<S, P>
where
    S: ShutdownProvider,
    P: PerfProvider + 'static,
{
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            RunnerProtoError::Proto(ref e) => Some(e),
            RunnerProtoError::Shutdown(ref e) => Some(e),
            RunnerProtoError::Taskcluster(ref e) => Some(e),
            RunnerProtoError::WaitForIdle(ref e) => Some(e),
            RunnerProtoError::Zip(ref e) => Some(e),
            RunnerProtoError::MissingFirefox => None,
            RunnerProtoError::EmptyProfile => None,
        }
    }
}

impl<S, P> From<ProtoError<RecorderMessageKind>> for RunnerProtoError<S, P>
where
    S: ShutdownProvider,
    P: PerfProvider,
{
    fn from(e: ProtoError<RecorderMessageKind>) -> Self {
        RunnerProtoError::Proto(e)
    }
}

impl<S, P> From<TaskclusterError> for RunnerProtoError<S, P>
where
    S: ShutdownProvider,
    P: PerfProvider,
{
    fn from(e: TaskclusterError) -> Self {
        RunnerProtoError::Taskcluster(e)
    }
}

impl<S, P> From<ZipError> for RunnerProtoError<S, P>
where
    S: ShutdownProvider,
    P: PerfProvider,
{
    fn from(e: ZipError) -> Self {
        RunnerProtoError::Zip(e)
    }
}

impl<S, P> From<io::Error> for RunnerProtoError<S, P>
where
    S: ShutdownProvider,
    P: PerfProvider,
{
    fn from(e: io::Error) -> Self {
        RunnerProtoError::Proto(ProtoError::Io(e))
    }
}
