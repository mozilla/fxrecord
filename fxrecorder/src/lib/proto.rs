// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::io;
use std::path::Path;

use libfxrecord::error::ErrorMessage;
use libfxrecord::net::*;
use libfxrecord::prefs::PrefValue;
use slog::{error, info, Logger};
use thiserror::Error;
use tokio::fs::File;
use tokio::net::TcpStream;

/// The recorder side of the protocol.
pub struct RecorderProto {
    inner: Option<Proto<RunnerMessage, RecorderMessage, RunnerMessageKind, RecorderMessageKind>>,
    log: Logger,
}

impl RecorderProto {
    /// Create a new RecorderProto.
    pub fn new(log: Logger, stream: TcpStream) -> RecorderProto {
        Self {
            inner: Some(Proto::new(stream)),
            log,
        }
    }

    /// Send a new request to the runner.
    pub async fn send_new_request(
        &mut self,
        task_id: &str,
        profile_path: Option<&Path>,
        prefs: Vec<(String, PrefValue)>,
    ) -> Result<(), RecorderProtoError> {
        info!(self.log, "Sending request");

        let profile_size = match profile_path {
            None => None,
            Some(profile_path) => Some(tokio::fs::metadata(profile_path).await?.len()),
        };

        self.send::<Request>(
            NewRequest {
                build_task_id: task_id.into(),
                profile_size,
                prefs,
            }
            .into(),
        )
        .await?;

        loop {
            let DownloadBuild { result } = self.recv().await?;

            match result {
                Ok(DownloadStatus::Downloading) => {
                    info!(self.log, "Downloading build ...");
                }

                Ok(DownloadStatus::Downloaded) => {
                    info!(self.log, "Build download complete; extracting build ...");
                }

                Ok(DownloadStatus::Extracted) => {
                    info!(self.log, "Build extracted");
                    break;
                }

                Err(e) => {
                    error!(self.log, "Build download failed"; "task_id" => task_id, "error" => ?e);
                    return Err(e.into());
                }
            }
        }

        if let Some(profile_path) = profile_path {
            self.send_profile(profile_path, profile_size.unwrap())
                .await?
        } else {
            info!(self.log, "No profile to send");
            if let Err(e) = self.recv::<CreateProfile>().await?.result {
                error!(self.log, "Runner could not create profile"; "error" => ?e);
                return Err(e.into());
            }
        }

        if let WritePrefs { result: Err(e) } = self.recv().await? {
            error!(self.log, "Runner could not write prefs"; "error" => ?e);
            return Err(e.into());
        }

        if let Restarting { result: Err(e) } = self.recv().await? {
            error!(self.log, "Runner could not restart"; "error" => ?e);
            return Err(e.into());
        }

        info!(self.log, "Runner is restarting...");

        Ok(())
    }

    /// Send a resume request to the runner.
    pub async fn send_resume_request(&mut self, idle: Idle) -> Result<(), RecorderProtoError> {
        info!(self.log, "Resuming request");
        self.send::<Request>(ResumeRequest { idle }.into()).await?;

        if let ResumeResponse { result: Err(e) } = self.recv().await? {
            error!(self.log, "Could not resume request with runner"; "error" => ?e);
            return Err(e.into());
        }

        if idle == Idle::Wait {
            info!(self.log, "Waiting for runner to become idle...");

            if let WaitForIdle { result: Err(e) } = self.recv().await? {
                error!(self.log, "Runner could not become idle"; "error" => ?e);
                return Err(e.into());
            }

            info!(self.log, "Runner became idle");
        }

        Ok(())
    }

    /// Send the profile at the given path to the runner.
    async fn send_profile(
        &mut self,
        profile_path: &Path,
        profile_size: u64,
    ) -> Result<(), RecorderProtoError> {
        let RecvProfile { result } = self.recv().await?;

        match result? {
            DownloadStatus::Downloading => {
                info!(self.log, "Sending profile"; "profile_size" => profile_size);
            }

            unexpected => {
                return Err(RecorderProtoError::RecvProfileMismatch {
                    received: unexpected,
                    expected: DownloadStatus::Downloading,
                });
            }
        }

        let mut stream = self.inner.take().unwrap().into_inner();
        let result = RecorderProto::send_profile_impl(&mut stream, profile_path).await;
        self.inner = Some(Proto::new(stream));

        result?;

        let mut state = DownloadStatus::Downloading;
        loop {
            let next_state = self.recv::<RecvProfile>().await?.result?;

            assert_ne!(state, DownloadStatus::Extracted);
            let expected = state.next().unwrap();

            if expected != next_state {
                return Err(RecorderProtoError::RecvProfileMismatch {
                    received: next_state,
                    expected,
                });
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

        assert!(state == DownloadStatus::Extracted);

        Ok(())
    }

    /// Write the raw bytes from the profile to the runner.
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
}

#[derive(Debug, Error)]
pub enum RecorderProtoError {
    #[error(transparent)]
    Proto(#[from] ProtoError<RunnerMessageKind>),

    #[error(
        "Expected a download status of `{}', but received `{}' instead",
        expected,
        received
    )]
    RecvProfileMismatch {
        expected: DownloadStatus,
        received: DownloadStatus,
    },
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
