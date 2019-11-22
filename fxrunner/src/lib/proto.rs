// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::error::Error;
use std::path::{Path, PathBuf};

use derive_more::Display;
use libfxrecord::error::ErrorExt;
use libfxrecord::net::*;
use slog::{error, info, Logger};
use tokio::net::TcpStream;

use crate::shutdown::ShutdownProvider;
use crate::taskcluster::{Taskcluster, TaskclusterError};

/// The runner side of the protocol.
pub struct RunnerProto<S> {
    inner: Proto<RecorderMessage, RunnerMessage, RecorderMessageKind, RunnerMessageKind>,
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
            inner: Proto::new(stream),
            log,
            shutdown_handler,
            tc,
        }
    }

    /// Consume the RunnerProto and return the underlying `Proto`.
    pub fn into_inner(
        self,
    ) -> Proto<RecorderMessage, RunnerMessage, RecorderMessageKind, RunnerMessageKind> {
        self.inner
    }

    /// Handshake with FxRecorder.
    pub async fn handshake_reply(&mut self) -> Result<bool, RunnerProtoError<S::Error>> {
        info!(self.log, "Handshaking ...");
        let Handshake { restart } = self.inner.recv().await?;

        if restart {
            if let Err(e) = self
                .shutdown_handler
                .initiate_restart("fxrecord: recorder requested restart")
            {
                error!(self.log, "an error occurred while handshaking"; "error" => ?e);
                self.inner
                    .send(HandshakeReply {
                        result: Err(e.into_error_message()),
                    })
                    .await?;

                return Err(RunnerProtoError::Shutdown(e));
            }
            info!(self.log, "Restart requested; restarting ...");
        }

        self.inner.send(HandshakeReply { result: Ok(()) }).await?;
        info!(self.log, "Handshake complete");

        Ok(restart)
    }

    pub async fn download_build_reply(
        &mut self,
        download_dir: &Path,
    ) -> Result<PathBuf, RunnerProtoError<S::Error>> {
        let DownloadBuild { task_id } = self.inner.recv().await?;

        info!(self.log, "Received build download request"; "task_id" => &task_id);

        self.inner
            .send(DownloadBuildReply { result: Ok(false) })
            .await?;

        match self
            .tc
            .download_build_artifact(&task_id, download_dir)
            .await
        {
            Ok(download) => {
                self.inner
                    .send(DownloadBuildReply { result: Ok(true) })
                    .await?;

                Ok(download)
            }

            Err(e) => {
                error!(self.log, "could not download build"; "error" => %e);
                self.inner
                    .send(DownloadBuildReply {
                        result: Err(e.into_error_message()),
                    })
                    .await?;
                Err(e.into())
            }
        }
    }
}

#[derive(Debug, Display)]
pub enum RunnerProtoError<S> {
    Proto(ProtoError<RecorderMessageKind>),
    Shutdown(S),
    Taskcluster(TaskclusterError),
}

impl<S> Error for RunnerProtoError<S>
where
    S: Error + 'static,
{
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            RunnerProtoError::Proto(ref e) => Some(e),
            RunnerProtoError::Shutdown(ref e) => Some(e),
            RunnerProtoError::Taskcluster(ref e) => Some(e),
        }
    }
}

impl<S> From<ProtoError<RecorderMessageKind>> for RunnerProtoError<S>
where
    S: Error + 'static,
{
    fn from(e: ProtoError<RecorderMessageKind>) -> Self {
        RunnerProtoError::Proto(e)
    }
}

impl<S> From<TaskclusterError> for RunnerProtoError<S>
where
    S: Error + 'static,
{
    fn from(e: TaskclusterError) -> Self {
        RunnerProtoError::Taskcluster(e)
    }
}
