// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use libfxrecord::net::*;
use slog::{error, info, Logger};
use tokio::net::TcpStream;

pub type ProtoError = libfxrecord::net::ProtoError<RunnerMessageKind>;

/// The recorder side of the protocol.
pub struct RecorderProto {
    inner: Proto<RunnerMessage, RecorderMessage, RunnerMessageKind, RecorderMessageKind>,
    log: Logger,
}

impl RecorderProto {
    pub fn new(log: Logger, stream: TcpStream) -> RecorderProto {
        Self {
            inner: Proto::new(stream),
            log,
        }
    }

    /// Consume the RecorderProto and return the underlying `Proto`.
    pub fn into_inner(
        self,
    ) -> Proto<RunnerMessage, RecorderMessage, RunnerMessageKind, RecorderMessageKind> {
        self.inner
    }

    /// Handshake with FxRunner.
    pub async fn handshake(&mut self, restart: bool) -> Result<(), ProtoError> {
        info!(self.log, "Handshaking ...");
        self.inner.send(Handshake { restart }).await?;
        let HandshakeReply { result } = self.inner.recv().await?;

        match result {
            Ok(..) => {
                info!(self.log, "Handshake complete");
                Ok(())
            }
            Err(e) => {
                info!(self.log, "Handshake failed: runner could not restart"; "error" => ?e);
                Err(ProtoError::Foreign(e))
            }
        }
    }

    pub async fn download_build(&mut self, task_id: &str) -> Result<(), ProtoError> {
        info!(self.log, "Requesting download of build from task"; "task_id" => task_id);
        self.inner
            .send(DownloadBuild {
                task_id: task_id.into(),
            })
            .await?;

        loop {
            let DownloadBuildReply { result } = self.inner.recv().await?;

            match result {
                Ok(DownloadStatus::Downloading) => {
                    info!(self.log, "Downloading build ...");
                }

                Ok(DownloadStatus::Downloaded) => {
                    info!(self.log, "Build download complete; extracting build ...");
                }

                Ok(DownloadStatus::Extracted) => {
                    info!(self.log, "Build extracted");
                    return Ok(());
                }

                Err(e) => {
                    error!(self.log, "Build download failed"; "task_id" => task_id, "error" => ?e);
                    return Err(ProtoError::Foreign(e));
                }
            }
        }
    }
}
