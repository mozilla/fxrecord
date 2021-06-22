// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::error::Error;
use std::fmt::Debug;
use std::io;
use std::path::{Path, PathBuf};

use libfxrecord::error::ErrorMessage;
use libfxrecord::net::*;
use libfxrecord::prefs::PrefValue;
use slog::{error, info, warn, Logger};
use thiserror::Error;
use tokio::fs::File;
use tokio::net::TcpStream;

use crate::recorder::Recorder;

/// The recorder side of the protocol.
pub struct RecorderProto<R> {
    inner: Option<Proto<RunnerMessage, RecorderMessage, RunnerMessageKind, RecorderMessageKind>>,
    log: Logger,
    recorder: R,
}

impl<R> RecorderProto<R>
where
    R: Recorder,
{
    /// Create a new RecorderProto.
    pub fn new(log: Logger, stream: TcpStream, recorder: R) -> Self {
        Self {
            inner: Some(Proto::new(stream)),
            log,
            recorder,
        }
    }

    /// Send a request for a new session to the runner.
    pub async fn new_session(
        &mut self,
        task_id: &str,
        profile_path: Option<&Path>,
        prefs: &[(String, PrefValue)],
    ) -> Result<String, RecorderProtoError<R::Error>> {
        info!(self.log, "Requesting new session");

        let profile_size = match profile_path {
            None => None,
            Some(profile_path) => Some(tokio::fs::metadata(profile_path).await?.len()),
        };

        self.send::<Session>(
            NewSessionRequest {
                build_task_id: task_id.into(),
                profile_size,
                prefs: Vec::from(prefs),
            }
            .into(),
        )
        .await?;

        let session_id = match self.recv::<NewSessionResponse>().await?.session_id {
            Ok(session_id) => session_id,
            Err(e) => {
                error!(self.log, "runner could not create new session"; "error" => %e);
                return Err(e.into());
            }
        };

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
                    error!(self.log, "Build download failed"; "task_id" => task_id, "error" => %e);
                    return Err(e.into());
                }
            }
        }

        if let DisableUpdates { result: Err(e) } = self.recv().await? {
            error!(self.log, "Runner could not disable updates"; "error" => %e);
            return Err(e.into());
        }

        if let Some(profile_path) = profile_path {
            self.send_profile(profile_path, profile_size.unwrap())
                .await?
        } else {
            info!(self.log, "No profile to send");
            if let Err(e) = self.recv::<CreateProfile>().await?.result {
                error!(self.log, "Runner could not create profile"; "error" => %e);
                return Err(e.into());
            }
        }

        if let WritePrefs { result: Err(e) } = self.recv().await? {
            error!(self.log, "Runner could not write prefs"; "error" => %e);
            return Err(e.into());
        }

        if let Restarting { result: Err(e) } = self.recv().await? {
            error!(self.log, "Runner could not restart"; "error" => %e);
            return Err(e.into());
        }

        info!(self.log, "Runner is restarting...");

        Ok(session_id)
    }

    /// Send a request to resume a session to the runner.
    pub async fn resume_session(
        &mut self,
        session_id: &str,
        idle: Idle,
        directory: &Path,
    ) -> Result<PathBuf, RecorderProtoError<R::Error>> {
        info!(self.log, "Resuming session");
        self.send::<Session>(
            ResumeSessionRequest {
                session_id: session_id.into(),
                idle,
            }
            .into(),
        )
        .await?;

        if let ResumeResponse { result: Err(e) } = self.recv().await? {
            error!(
                self.log,
                "Could not resume session with runner";
                "id" => session_id,
                "error" => %e,
            );
            return Err(e.into());
        }

        if idle == Idle::Wait {
            info!(self.log, "Waiting for runner to become idle...");

            if let WaitForIdle { result: Err(e) } = self.recv().await? {
                error!(self.log, "Runner could not become idle"; "error" => %e);
                return Err(e.into());
            }

            info!(self.log, "Runner became idle");
        }

        info!(self.log, "Beginning recording...");
        let handle = self
            .recorder
            .start_recording(directory)
            .await
            .map_err(RecorderProtoError::Recording)?;

        info!(self.log, "requesting Firefox start...");
        self.send(StartFirefox).await?;
        if let Err(e) = self.recv::<StartedFirefox>().await?.result {
            error!(self.log, "recorder could not launch firefox"; "error" => %e);
            return Err(e.into());
        }
        info!(self.log, "runner started Firefox.");

        let recording_path = self
            .recorder
            .wait_for_recording_finished(handle)
            .await
            .map_err(RecorderProtoError::Recording)?;

        info!(self.log, "requesting runner stop Firefox...");
        self.send(StopFirefox).await?;

        if let Err(errors) = self.recv::<StoppedFirefox>().await?.result {
            if errors.len() > 1 {
                for error in &errors {
                    warn!(
                        self.log,
                        "recorder could not stop firefox (multiple errors)";
                        "error" => %error
                    );
                }
            } else {
                assert!(!errors.is_empty());
                warn!(
                    self.log,
                    "recorder could not stop Firefox";
                    "error" => %errors[0]
                );
            }
        }

        info!(self.log, "runner stopped Firefox");

        if let Err(e) = self.recv::<SessionFinished>().await?.result {
            warn!(self.log, "runner did not clean up successfully"; "error" => ?e);
        }

        info!(self.log, "recording complete");

        Ok(recording_path)
    }

    /// Send the profile at the given path to the runner.
    async fn send_profile(
        &mut self,
        profile_path: &Path,
        profile_size: u64,
    ) -> Result<(), RecorderProtoError<R::Error>> {
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
        let result = Self::send_profile_impl(&mut stream, profile_path).await;
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
    ) -> Result<(), RecorderProtoError<R::Error>> {
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

/// An error in the RecordingProto.
///
/// For a `RecordingProto<R: Recorder>`, `RecordingError` is `<R as Recorder>::Error`.
#[derive(Debug, Error)]
pub enum RecorderProtoError<RecordingError>
where
    RecordingError: Error + 'static,
{
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

    #[error(transparent)]
    Recording(RecordingError),
}

impl<RecordingError> From<ErrorMessage<String>> for RecorderProtoError<RecordingError>
where
    RecordingError: Error + 'static,
{
    fn from(e: ErrorMessage<String>) -> Self {
        RecorderProtoError::Proto(ProtoError::from(e))
    }
}

impl<RecordingError> From<io::Error> for RecorderProtoError<RecordingError>
where
    RecordingError: Error + 'static,
{
    fn from(e: io::Error) -> Self {
        RecorderProtoError::Proto(ProtoError::from(e))
    }
}
