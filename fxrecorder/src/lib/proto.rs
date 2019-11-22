// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::error::Error;
use std::io;
use std::path::Path;

use derive_more::Display;
use libfxrecord::error::ErrorMessage;
use libfxrecord::net::*;
use slog::{error, info, Logger};
use tokio::fs::File;
use tokio::net::TcpStream;

/// The recorder side of the protocol.
pub struct RecorderProto {
    inner: Option<Proto<RunnerMessage, RecorderMessage, RunnerMessageKind, RecorderMessageKind>>,
    log: Logger,
}

impl RecorderProto {
    pub fn new(log: Logger, stream: TcpStream) -> RecorderProto {
        Self {
            inner: Some(Proto::new(stream)),
            log,
        }
    }

    /// Consume the RecorderProto and return the underlying `Proto`.
    pub fn into_inner(
        self,
    ) -> Proto<RunnerMessage, RecorderMessage, RunnerMessageKind, RecorderMessageKind> {
        self.inner.unwrap()
    }

    /// Send the given message to the recorder.
    ///
    /// If the underlying proto is None, this will panic.
    async fn send<M>(&mut self, m: M) -> Result<(), ProtoError<RunnerMessageKind>>
    where
        for<'de> M: MessageContent<'de, RecorderMessage, RecorderMessageKind>,
    {
        self.inner.as_mut().unwrap().send(m).await
    }

    /// Receive a given kind of message from the recorder.
    ///
    /// If the underlying proto is None, this will panic.
    async fn recv<M>(&mut self) -> Result<M, ProtoError<RunnerMessageKind>>
    where
        for<'de> M: MessageContent<'de, RunnerMessage, RunnerMessageKind>,
    {
        self.inner.as_mut().unwrap().recv::<M>().await
    }

    /// Handshake with FxRunner.
    pub async fn handshake(&mut self, restart: bool) -> Result<(), RecorderProtoError> {
        info!(self.log, "Handshaking ...");
        self.send(Handshake { restart }).await?;
        let HandshakeReply { result } = self.recv().await?;

        match result {
            Ok(..) => {
                info!(self.log, "Handshake complete");
                Ok(())
            }
            Err(e) => {
                info!(self.log, "Handshake failed: runner could not restart"; "error" => ?e);
                Err(e.into())
            }
        }
    }

    pub async fn download_build(&mut self, task_id: &str) -> Result<(), RecorderProtoError> {
        info!(self.log, "Requesting download of build from task"; "task_id" => task_id);
        self.send(DownloadBuild {
            task_id: task_id.into(),
        })
        .await?;

        loop {
            let DownloadBuildReply { result } = self.recv().await?;

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
                    return Err(e.into());
                }
            }
        }
    }

    /// Send the profile at the given path to the runner.
    ///
    /// If the profile path is specified, the profile must exist, or this function will panic.
    pub async fn send_profile(
        &mut self,
        profile_path: Option<&Path>,
    ) -> Result<(), RecorderProtoError> {
        let profile_path = match profile_path {
            Some(profile_path) => profile_path,

            None => {
                info!(self.log, "No profile to send");

                self.send(SendProfile { profile_size: None }).await?;
                let SendProfileReply { result } = self.recv().await?;

                return match result? {
                    Some(unexpected) => Err(RecorderProtoError::SendProfileMismatch {
                        expected: None,
                        received: Some(unexpected),
                    }
                    .into()),

                    None => Ok(()),
                };
            }
        };

        assert!(profile_path.exists());
        let profile_size = tokio::fs::metadata(profile_path).await?.len();

        self.send(SendProfile {
            profile_size: Some(profile_size),
        })
        .await?;

        let SendProfileReply { result } = self.recv().await?;

        match result? {
            Some(DownloadStatus::Downloading) => {
                info!(self.log, "Sending profile"; "profile_size" => profile_size);
            }

            unexpected => {
                return Err(RecorderProtoError::SendProfileMismatch {
                    received: unexpected,
                    expected: Some(DownloadStatus::Downloading),
                }
                .into())
            }
        }

        let mut stream = self.inner.take().unwrap().into_inner();
        let result = RecorderProto::send_profile_impl(&mut stream, profile_path).await;
        self.inner = Some(Proto::new(stream));

        result?;

        let mut state = DownloadStatus::Downloading;
        loop {
            let SendProfileReply { result } = self.recv().await?;

            match result? {
                Some(next_state) => {
                    assert_ne!(state, DownloadStatus::Extracted);
                    let expected = state.next().unwrap();

                    if expected != next_state {
                        return Err(RecorderProtoError::SendProfileMismatch {
                            received: Some(next_state),
                            expected: Some(expected),
                        }
                        .into());
                    }

                    state = next_state;

                    match state {
                        // This would be caught above because this is never an expected state.
                        DownloadStatus::Downloading => unreachable!(),

                        DownloadStatus::Downloaded => {
                            info!(self.log, "Profile sent; extracting...");
                        }

                        DownloadStatus::Extracted => {
                            info!(self.log, "Profile extracted");
                            break;
                        }
                    }
                }

                None => {
                    return Err(RecorderProtoError::SendProfileMismatch {
                        received: None,
                        expected: state.next(),
                    }
                    .into())
                }
            }
        }

        assert!(state == DownloadStatus::Extracted);

        Ok(())
    }

    async fn send_profile_impl(
        stream: &mut TcpStream,
        profile_path: &Path,
    ) -> Result<(), RecorderProtoError> {
        let mut f = File::open(profile_path).await?;

        tokio::io::copy(&mut f, stream)
            .await
            .map_err(Into::into)
            .map(drop)
    }
}

#[derive(Debug, Display)]
pub enum RecorderProtoError {
    Proto(ProtoError<RunnerMessageKind>),

    #[display(
        fmt = "Expected a download status of `{:?}', but received `{:?}' instead",
        expected,
        received
    )]
    SendProfileMismatch {
        expected: Option<DownloadStatus>,
        received: Option<DownloadStatus>,
    },
}

impl Error for RecorderProtoError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            RecorderProtoError::Proto(ref e) => Some(e),
            RecorderProtoError::SendProfileMismatch { .. } => None,
        }
    }
}

impl From<ProtoError<RunnerMessageKind>> for RecorderProtoError {
    fn from(e: ProtoError<RunnerMessageKind>) -> Self {
        RecorderProtoError::Proto(e)
    }
}

impl From<ErrorMessage<String>> for RecorderProtoError {
    fn from(e: ErrorMessage<String>) -> Self {
        RecorderProtoError::Proto(ProtoError::from(e))
    }
}

impl From<io::Error> for RecorderProtoError {
    fn from(e: io::Error) -> Self {
        RecorderProtoError::Proto(ProtoError::from(e))
    }
}
