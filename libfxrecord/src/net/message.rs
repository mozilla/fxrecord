// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Message types used throughout `fxrunner` and `fxrecorder`.
//!
//! This module consists of various helper traits and structures, which when
//! combined with the [`message_type!`][message_type] macro from the
//! `libfxrecord_macros` crate, provide a convient way for [`Proto`][Proto]
//! instances to send and receive typed messages at a high level.
//!
//! [Proto]: ./struct.Proto.html
//! [message_type]: ../../../libfxrecord_macros/macro.message_type.html

use std::convert::TryFrom;
use std::fmt::{Debug, Display};

use derive_more::Display;
use libfxrecord_macros::message_type;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::error::ErrorMessage;
use crate::prefs::PrefValue;

/// A message is a serializable and deserializable type.
pub trait Message<'de>: Serialize + Deserialize<'de> + Unpin {
    /// Each message has a kind that uniquely identifies it.
    type Kind: Debug + Display;

    /// Return the kind of the message.
    fn kind(&self) -> Self::Kind;
}

/// A trait that links message contents to their message wrapper enums.
pub trait MessageContent<'de, M, K>:
    Serialize + Deserialize<'de> + Unpin + Into<M> + TryFrom<M, Error = KindMismatch<K>>
where
    M: Message<'de, Kind = K>,
    K: Debug + Display,
{
    /// Return the kind of the message.
    fn kind() -> K;
}

/// An error that occurs when attempting to extract a message variant.
#[derive(Debug, Error)]
#[error(
    "could not convert message of kind `{}' to kind `{}'",
    .expected,
    .actual
)]
pub struct KindMismatch<K: Debug + Display> {
    pub expected: K,
    pub actual: K,
}

impl From<NewSessionRequest> for Session {
    fn from(req: NewSessionRequest) -> Session {
        Session::NewSession(req)
    }
}

impl From<ResumeSessionRequest> for Session {
    fn from(req: ResumeSessionRequest) -> Session {
        Session::ResumeSession(req)
    }
}

/// Whether the runner should wait to become idle.
#[derive(Clone, Copy, Debug, Eq, Deserialize, PartialEq, Serialize)]
pub enum Idle {
    /// Wait to become idle.
    Wait,

    /// Skip waiting to become idle.
    Skip,
}

/// A request for a new session.
#[derive(Debug, Deserialize, Serialize)]
pub struct NewSessionRequest {
    /// The task ID of the Taskcluster build task.
    ///
    /// The build artifact from this task will be downloaded by the runner.
    pub build_task_id: String,

    /// The size of the profile that will be sent, if any.
    pub profile_size: Option<u64>,

    /// Prefs to override in the profile.
    pub prefs: Vec<(String, PrefValue)>,
}

/// A request to resume an existing session.
#[derive(Debug, Deserialize, Serialize)]
pub struct ResumeSessionRequest {
    /// The ID of the session being resumed.
    pub session_id: String,

    /// Whether or not the runner should wait for idle before running Firefox.
    pub idle: Idle,
}

#[derive(Debug, Display, Eq, PartialEq, Serialize, Deserialize)]
pub enum DownloadStatus {
    Downloading,
    Downloaded,
    Extracted,
}

impl DownloadStatus {
    /// Return the next expected state, if any.
    pub fn next(&self) -> Option<DownloadStatus> {
        match self {
            DownloadStatus::Downloading => Some(DownloadStatus::Downloaded),
            DownloadStatus::Downloaded => Some(DownloadStatus::Extracted),
            DownloadStatus::Extracted => None,
        }
    }
}

pub type ForeignResult<T> = Result<T, ErrorMessage<String>>;

message_type! {
    /// A message from FxRecorder to FxRunner.
    RecorderMessage,

    /// The kind of a [`RecorderMessage`](struct.RecorderMessage.html).
    RecorderMessageKind;

    /// A request from the recorder to the runner.
    pub enum Session {
        /// A request for a new session.
        ///
        /// If successful, the runner will restart and the recorder should send a
        /// [`ResumeSession`](enum.RecorderSession.html#variant.ResumeSession)
        /// upon reconnection.
        NewSession(NewSessionRequest),

        /// A request to resume a [previous
        /// request](enum.RecorderSession.html#variant.NewSession).
        ResumeSession(ResumeSessionRequest),
    }
}

message_type! {
    /// A message from FxRunner to FxRecorder.
    RunnerMessage,

    /// The kind of a [`RunnerMessage`](struct.RunnerMessage.html).
    RunnerMessageKind;

    /// The status of the DownloadBuild phase.
    pub struct DownloadBuild {
        pub result: ForeignResult<DownloadStatus>,
    }

    /// The status of the disable updates phase.
    pub struct DisableUpdates {
        pub result: ForeignResult<()>,
    }

    /// The status of the RecvProfile phase.
    pub struct RecvProfile {
        pub result: ForeignResult<DownloadStatus>,
    }

    /// The result of the CreateProfile phase.
    pub struct CreateProfile {
        pub result: ForeignResult<()>,
    }

    /// The status of the WritePrefs phase.
    pub struct WritePrefs {
        pub result: ForeignResult<()>,
    }

    /// The status of the Restarting phase.
    pub struct Restarting {
        pub result: ForeignResult<()>,
    }

    /// The status of the NewSession phase.
    pub struct NewSessionResponse {
        /// The session ID to be given in a
        /// [`ResumeSession`](enum.Session.html#variant.ResumeSession) message.
        pub session_id: ForeignResult<String>,
    }

    /// The status of the ResumeResponse phase.
    pub struct ResumeResponse {
        pub result: ForeignResult<()>,
    }

    /// The status of the WaitForIdle phase.
    pub struct WaitForIdle {
        pub result: ForeignResult<()>,
    }
}
