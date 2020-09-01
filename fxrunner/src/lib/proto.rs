// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::io;
use std::path::{Path, PathBuf};

use indoc::indoc;
use libfxrecord::error::ErrorExt;
use libfxrecord::net::*;
use libfxrecord::prefs::write_prefs;
use scopeguard::{guard, ScopeGuard};
use slog::{error, info, Logger};
use thiserror::Error;
use tokio::fs::{create_dir, rename, File, OpenOptions};
use tokio::net::TcpStream;
use tokio::prelude::*;
use tokio::task::spawn_blocking;

use crate::fs::PathExt;
use crate::osapi::{cpu_and_disk_idle, PerfProvider, ShutdownProvider, WaitForIdleError};
use crate::session::{
    cleanup_session, NewSessionError, ResumeSessionError, SessionInfo, SessionManager,
};
use crate::taskcluster::Taskcluster;
use crate::zip::{unzip, ZipError};

/// The runner side of the protocol.
pub struct RunnerProto<S, T, P, R> {
    inner: Option<Proto<RecorderMessage, RunnerMessage, RecorderMessageKind, RunnerMessageKind>>,
    log: Logger,
    shutdown_handler: S,
    tc: T,
    perf_provider: P,
    session_manager: R,
}

impl<S, T, P, R> RunnerProto<S, T, P, R>
where
    S: ShutdownProvider,
    T: Taskcluster,
    P: PerfProvider + 'static,
    R: SessionManager,
{
    /// Handle a request from the recorder.
    pub async fn handle_request(
        log: Logger,
        stream: TcpStream,
        shutdown_handler: S,
        tc: T,
        perf_provider: P,
        session_manager: R,
    ) -> Result<bool, RunnerProtoError<S, T, P>> {
        let mut proto = Self {
            inner: Some(Proto::new(stream)),
            log,
            shutdown_handler,
            tc,
            perf_provider,
            session_manager,
        };

        match proto.recv::<Session>().await? {
            Session::NewSession(req) => {
                proto.handle_new_session(req).await?;
                Ok(true)
            }

            Session::ResumeSession(req) => {
                proto.handle_resume_session(req).await?;
                Ok(false)
            }
        }
    }

    /// Handle a request for a new session from the recorder.
    async fn handle_new_session(
        &mut self,
        request: NewSessionRequest,
    ) -> Result<(), RunnerProtoError<S, T, P>> {
        let session_info = match self.session_manager.new_session().await {
            Ok(session_info) => session_info,
            Err(e) => {
                self.send(NewSessionResponse {
                    session_id: Err(e.into_error_message()),
                })
                .await?;
                return Err(e.into());
            }
        };

        let cleanup = guard(self.log.clone(), |log| cleanup_session(log, &session_info));

        self.send(NewSessionResponse {
            session_id: Ok(session_info.id.clone().into_owned()),
        })
        .await?;

        let firefox_bin = self
            .download_build(&session_info, &request.build_task_id)
            .await?;
        assert!(firefox_bin.is_file_async().await);

        if let Err(e) = self.disable_updates(&session_info).await {
            error!(self.log, "Could not disable updates for downloaded Firefox"; "error" => %e);
            self.send(DisableUpdates {
                result: Err(e.into_error_message()),
            })
            .await?;

            return Err(e);
        }
        self.send(DisableUpdates { result: Ok(()) }).await?;

        let profile_path = match request.profile_size {
            Some(profile_size) => self.recv_profile(&session_info, profile_size).await?,
            None => {
                info!(self.log, "Creating new empty profile");

                let profile_path = match self
                    .session_manager
                    .ensure_valid_profile_dir(&session_info)
                    .await
                {
                    Ok(profile_path) => profile_path,
                    Err(e) => {
                        self.send(CreateProfile {
                            result: Err(e.into_error_message()),
                        })
                        .await?;
                        return Err(RunnerProtoError::EnsureProfile(e));
                    }
                };
                self.send(CreateProfile { result: Ok(()) }).await?;

                profile_path
            }
        };
        assert!(profile_path.is_dir_async().await);

        if !request.prefs.is_empty() {
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

        if let Err(e) = self
            .shutdown_handler
            .initiate_restart("fxrunner: restarting for cold Firefox start")
        {
            error!(self.log, "Could not restart"; "error" => %e);
            self.send(Restarting {
                result: Err(e.into_error_message()),
            })
            .await?;

            return Err(RunnerProtoError::Shutdown(e));
        }

        self.send(Restarting { result: Ok(()) }).await?;

        drop(ScopeGuard::into_inner(cleanup));

        Ok(())
    }

    /// Resume a session from the recorder.
    async fn handle_resume_session(
        &mut self,
        request: ResumeSessionRequest,
    ) -> Result<(), RunnerProtoError<S, T, P>> {
        info!(self.log, "Received resumption request");

        let session_info = match self
            .session_manager
            .resume_session(&request.session_id)
            .await
        {
            Ok(session_info) => session_info,
            Err(e) => {
                self.send(ResumeResponse {
                    result: Err(e.into_error_message()),
                })
                .await?;
                return Err(e.into());
            }
        };

        let _cleanup = guard(self.log.clone(), |log| cleanup_session(log, &session_info));

        self.send(ResumeResponse { result: Ok(()) }).await?;

        if request.idle == Idle::Wait {
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
        }

        Ok(())
    }

    /// Download a build from taskcluster.
    async fn download_build<'a>(
        &mut self,
        session_info: &'a SessionInfo<'a>,
        task_id: &str,
    ) -> Result<PathBuf, RunnerProtoError<S, T, P>> {
        info!(self.log, "Download build from Taskcluster"; "task_id" => &task_id);
        self.send(DownloadBuild {
            result: Ok(DownloadStatus::Downloading),
        })
        .await?;

        let download_path = match self
            .tc
            .download_build_artifact(task_id, &session_info.path)
            .await
        {
            Ok(download_path) => download_path,
            Err(e) => {
                error!(self.log, "Could not download build"; "error" => %e);
                self.send(DownloadBuild {
                    result: Err(e.into_error_message()),
                })
                .await?;
                return Err(RunnerProtoError::Taskcluster(e));
            }
        };

        self.send(DownloadBuild {
            result: Ok(DownloadStatus::Downloaded),
        })
        .await?;
        info!(self.log, "Extracting downloaded artifact...");

        let unzip_result = spawn_blocking({
            let download_dir = PathBuf::from(&session_info.path);
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

        let firefox_path = session_info.path.join("firefox").join("firefox.exe");
        if !firefox_path.is_file_async().await {
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

    async fn disable_updates(
        &mut self,
        session_info: &SessionInfo<'_>,
    ) -> Result<(), RunnerProtoError<S, T, P>> {
        const DISABLE_UPDATE_POLICY: &[u8] = indoc!(
            br#"
            {
                "policies": {
                    "DisableAppUpdate": true
                }
            }
            "#
        );
        let distribution_dir = session_info.path.join("firefox").join("distribution");

        create_dir(&distribution_dir)
            .await
            .map_err(RunnerProtoError::DisableUpdates)?;

        OpenOptions::new()
            .create(true)
            .write(true)
            .open(distribution_dir.join("policies.json"))
            .await
            .map_err(RunnerProtoError::DisableUpdates)?
            .write_all(DISABLE_UPDATE_POLICY)
            .await
            .map_err(RunnerProtoError::DisableUpdates)?;

        Ok(())
    }

    /// Receive a profile from the recorder.
    async fn recv_profile(
        &mut self,
        session_info: &SessionInfo<'_>,
        profile_size: u64,
    ) -> Result<PathBuf, RunnerProtoError<S, T, P>> {
        info!(self.log, "Receiving profile...");
        self.send(RecvProfile {
            result: Ok(DownloadStatus::Downloading),
        })
        .await?;

        let mut stream = self.inner.take().unwrap().into_inner();
        let result = Self::recv_profile_raw(&mut stream, &session_info.path, profile_size).await;
        self.inner = Some(Proto::new(stream));

        let zip_path = match result {
            Ok(zip_path) => zip_path,
            Err(e) => {
                self.send(DownloadBuild {
                    result: Err(e.into_error_message()),
                })
                .await?;
                return Err(e);
            }
        };

        info!(self.log, "Profile received; extracting...");
        self.send(RecvProfile {
            result: Ok(DownloadStatus::Downloaded),
        })
        .await?;

        // It is possible that the profile contains a top-level directory, in
        // which case we don't want to directly extract to
        // `request_info.path.join("profile")`. Instead, we unzip it to a
        // temporary directory and then move the top level directory (which may
        // be the path we extracted it to) to the target profile directory.
        let unzip_path = session_info.path.join("unzipped_profile");

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
                error!(self.log, "Could not extract profile"; "error" => %e);

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

        let unzipped_profile_dir = stats.top_level_dir.unwrap_or(unzip_path);
        let profile_dir = session_info.path.join("profile");
        if let Err(e) = rename(unzipped_profile_dir, &profile_dir).await {
            error!(self.log, "Could not rename profile directory after extraction"; "error" => %e);

            self.send(RecvProfile {
                result: Err(e.into_error_message()),
            })
            .await?;

            return Err(e.into());
        }

        info!(self.log, "Profile extracted");

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
    ) -> Result<PathBuf, RunnerProtoError<S, T, P>> {
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

#[derive(Debug, Error)]
pub enum RunnerProtoError<S, T, P>
where
    S: ShutdownProvider,
    T: Taskcluster,
    P: PerfProvider + 'static,
{
    #[error("An empty profile was received")]
    EmptyProfile,

    #[error("No firefox.exe in build artifact")]
    MissingFirefox,

    #[error(transparent)]
    Proto(#[from] ProtoError<RecorderMessageKind>),

    #[error(transparent)]
    Shutdown(S::Error),

    #[error("Could not disable updates: {}", .0)]
    DisableUpdates(#[source] io::Error),

    #[error(transparent)]
    Taskcluster(T::Error),

    #[error(transparent)]
    WaitForIdle(WaitForIdleError<P>),

    #[error(transparent)]
    Zip(#[from] ZipError),

    #[error(transparent)]
    NewSession(#[from] NewSessionError),

    #[error(transparent)]
    ResumeSession(#[from] ResumeSessionError),

    #[error(transparent)]
    EnsureProfile(io::Error),
}

impl<S, T, P> From<io::Error> for RunnerProtoError<S, T, P>
where
    S: ShutdownProvider,
    T: Taskcluster,
    P: PerfProvider,
{
    fn from(e: io::Error) -> Self {
        RunnerProtoError::Proto(ProtoError::Io(e))
    }
}
