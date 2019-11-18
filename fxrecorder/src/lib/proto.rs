// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use libfxrecord::net::*;
use slog::{info, Logger};
use tokio::net::TcpStream;

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
    pub async fn handshake(&mut self, restart: bool) -> Result<(), ProtoError<RunnerMessageKind>> {
        info!(self.log, "Handshaking ...");
        self.inner.send(Handshake { restart }).await?;
        self.inner.recv::<HandshakeReply>().await?;
        info!(self.log, "Handshake complete");
        Ok(())
    }
}