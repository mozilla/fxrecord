// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::error::Error;

use derive_more::Display;
use libfxrecord::error::ErrorExt;
use libfxrecord::net::*;
use slog::{error, info, Logger};
use tokio::net::TcpStream;

use crate::shutdown::ShutdownProvider;

pub type ProtoError = libfxrecord::net::ProtoError<RecorderMessageKind>;

/// The runner side of the protocol.
pub struct RunnerProto<S> {
    inner: Proto<RecorderMessage, RunnerMessage, RecorderMessageKind, RunnerMessageKind>,
    log: Logger,
    shutdown_handler: S,
}

impl<S> RunnerProto<S>
where
    S: ShutdownProvider,
{
    pub fn new(log: Logger, stream: TcpStream, shutdown_handler: S) -> Self {
        Self {
            inner: Proto::new(stream),
            log,
            shutdown_handler,
        }
    }

    /// Consume the RunnerProto and return the underlying `Proto`.
    pub fn into_inner(
        self,
    ) -> Proto<RecorderMessage, RunnerMessage, RecorderMessageKind, RunnerMessageKind> {
        self.inner
    }

    /// Handshake with FxRecorder.
    pub async fn handshake_reply(&mut self) -> Result<bool, HandshakeError<S::Error>> {
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

                return Err(HandshakeError::Shutdown(e));
            }
            info!(self.log, "Restart requested; restarting ...");
        }

        self.inner.send(HandshakeReply { result: Ok(()) }).await?;
        info!(self.log, "Handshake complete");

        Ok(restart)
    }
}

/// An error that occurs while handshaking.
#[derive(Debug, Display)]
pub enum HandshakeError<E: Error + Sized + 'static> {
    /// An underlying protocol error.
    Proto(ProtoError),
    /// An error that occurs when failing to shutdown.
    Shutdown(E),
}

impl<E: Error + 'static> Error for HandshakeError<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            HandshakeError::Proto(ref source) => Some(source),
            HandshakeError::Shutdown(ref source) => Some(source),
        }
    }
}

impl<E: Error> From<ProtoError> for HandshakeError<E> {
    fn from(e: ProtoError) -> Self {
        HandshakeError::Proto(e)
    }
}
