// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use slog::{info, Logger};
use tokio::net::TcpStream;

use libfxrecord::net::*;

/// The runner side of the protocol.
pub struct RunnerProto {
    inner: Proto<RecorderMessage, RunnerMessage, RecorderMessageKind, RunnerMessageKind>,
    log: Logger,
}

impl RunnerProto {
    pub fn new(log: Logger, stream: TcpStream) -> RunnerProto {
        Self {
            inner: Proto::new(stream),
            log,
        }
    }

    /// Handshake with FxRecorder.
    pub async fn handshake_reply(&mut self) -> Result<(), ProtoError<RecorderMessageKind>> {
        info!(self.log, "Handshaking ...");
        self.inner.recv::<Handshake>().await?;
        self.inner.send(HandshakeReply).await?;
        info!(self.log, "Handshake complete");
        Ok(())
    }
}
