// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Message types used throughout `fxrunner` and `fxrecorder`.
//!
//! This module consists of various helper traits and structures, as well as
//! the [`impl_message!`][impl_message] macro, which together provide a
//! convenient way for [`Proto`][Proto] instances to send and receive typed
//! messages at a high level.
//!
//! Each invocation of [`impl_message!`][impl_message] will generate several types:
//!
//! 1. The message type. This is a wrapper enum type that contains all message
//!    variants. It is the type that is serialized/deserialized by the
//!    [`Proto`][Proto]. It will also implement the [`Message`][Message] so that
//!    its variants can be differentiated by the message kind type.
//!
//!    For example, for this invocation of [`impl_message!`][impl_message]
//!
//!    ```ignore
//!    impl_message! {
//!        /// The message type.
//!        Msg,
//!        /// The message kind enumeration.
//!        MsgKind;
//!        /// A message without any fields.
//!        ///
//!        /// This is the most simple kind of message.
//!        FooMsg;
//!
//!        /// A "Bar msg"
//!        ///
//!        /// This message has a field.
//!        BarMsg {
//!            /// The contents of the message.
//!            ///
//!            /// The type of this field is `u32`.
//!            field: u32,
//!        };
//!    }
//!    ```
//!
//!    the following message type would be generated:
//!
//!    ```ignore
//!    pub enum Msg {
//!        FooMsg(FooMsg),
//!        BarMsg(BarMsg),
//!    }
//!
//!    impl Message<'_> for Msg { /* ... */ }
//!    ```
//!
//!    Here `FooMsg` and `BarMsg` will be generated structures containing actual
//!    message data.
//!
//! 2. A message kind type. This is an enum with one variant for each kind of message.
//!
//!    For example, for the same invocation of `impl_message!` as before, the
//!    following kind type would be generated:
//!
//!    ```ignore
//!    pub enum MsgKind {
//!        FooMsg,
//!        BarMsg,
//!    }
//!    ```
//!
//! 3. Message content types for each message.
//!
//!    In the example above, this is the `FooMsg` and `BarMsg` structures. The
//!    following would be generated for that invocation:
//!
//!    ```ignore
//!     pub struct FooMsg;
//!
//!     pub struct BarMsg {
//!         pub enum field: u32,
//!     }
//!     ```
//!
//!     as well as implementations for [`MessageContent`][MessageContent] and
//!     conversion traits.
//!
//!     These are the concrete message types that
//!     [`Proto::recv_kind`][Proto::recv_kind] will receive.
//!
//! [Proto]: ../proto/struct.Proto.html
//! [Proto::recv_kind]: ../proto/struct.Proto.html#fn.recv_kind
//! [Message]: trait.Message.html
//! [MessageContent]: trait.MessageContent.html
//! [impl_message]: ../../macro.impl_message.html

use std::convert::TryFrom;
use std::fmt::{Debug, Display};

use derive_more::Display;
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

/// Generate an inner message type.
///
/// The generated type will either be a unit struct or a non-empty struct with
/// named fields.
#[macro_export] // Only exported for doctests.
macro_rules! impl_message_inner {
    // Generate a unit struct.
    (
        $(#[doc = $doc:expr])*
        $name:ident
    ) => {
        $(#[doc = $doc])*
        #[derive(Debug, Deserialize, Serialize)]
        pub struct $name;
    };

    // Generate a struct with named fields.
    (
        $(#[doc = $doc:expr])*
        $name:ident {
        $(
            $(#[doc = $field_doc:expr])*
            $field:ident : $field_ty:ty,
        )*
    }) => {
        $(#[doc = $doc])*
        #[derive(Debug, Deserialize, Serialize)]
        pub struct $name {
            $(
                $(#[doc = $field_doc])?
                pub $field: $field_ty,
            )*
        }
    };
}

/// Generate messages and their implementations.
///
/// The first argument is the name of the message type. This will generate a
/// wrapper enum with this name that contains tuple variants for each named message.
///
/// The second argument is the name of the message kind type. This will generate
/// an enum with unit variants for each named message.
///
/// The rest of the arguments are the message variants, which will generate both
/// an enum variant in the message type and a standalone struct.
///
/// Doc-comments applied to items are persisted.
///
/// This will take a macro of the form:
///
/// ```
/// # #[macro_use] extern crate libfxrecord;
/// # use std::convert::{TryFrom, TryInto};
/// # use derive_more::Display;
/// # use serde::{Serialize, Deserialize};
/// # use libfxrecord::net::message::*;
/// impl_message! {
///     Msg,
///     MsgKind;
///     FooMsg;
///     BarMsg {
///         field: u32,
///     };
/// }
///
/// # let _m: FooMsg = Msg::FooMsg(FooMsg).try_into().unwrap();
/// # assert_eq!(FooMsg::kind(), MsgKind::FooMsg);
/// # assert_eq!(Msg::FooMsg(FooMsg).kind(), MsgKind::FooMsg);
/// #
/// # let _m: BarMsg = Msg::BarMsg(BarMsg { field: 1 }).try_into().unwrap();
/// # assert_eq!(BarMsg::kind(), MsgKind::BarMsg);
/// # assert_eq!(Msg::BarMsg(BarMsg { field: 1 }).kind(), MsgKind::BarMsg);
/// ```
///
/// and generate:
///
/// ```ignore
/// pub enum MsgKind {
///     FooMsg,
///     BarMsg,
/// }
///
/// pub enum Msg {
///     FooMsg(FooMsg),
///     BarMsg(BarMsg),
/// }
///
/// pub struct FooMsg;
///
/// pub struct BarMsg {
///     pub field: u32,
/// }
///
/// impl Message for Msg {
///     type Kind = MsgKind;
///
///     fn kind(&self) -> MsgKind {
///         match self {
///             Msg::FooMsg(..) => MsgKind::FooMsg,
///             Msg::BarMsg(..) => MsgKind::BarMsg,
///         }
///     }
/// }
///
/// impl Into<Msg> for FooMsg { /* ... */ }
/// impl TryFrom<Msg> for FooMsg { /* ... */ }
/// impl MessageContent<'_, Msg, MsgKind> for FooMsg { /* ... */ }
///
/// impl TryFrom<Msg> for BarMsg { /*... */ }
/// impl Into<Msg> for BarMsg { /* ... */ }
/// impl MessageContent<'_, Msg, MsgKind> for BarMsg { /* ... */ }
/// ```
#[macro_export] // Only exported for doctests.
macro_rules! impl_message {
    (
        $(#[doc = $msg_doc:expr])*
        $msg_ty:ident,

        $(#[doc = $kind_doc:expr])*
        $kind_ty:ident;

        $(
            $(#[doc = $inner_ty_doc:expr])*
            $inner_ty:ident $({
                $(
                    $(#[doc = $field_doc:expr])*
                    $field:ident: $field_ty:ty,
                )*
            })?;
        )*
    ) => {
        $(#[doc = $kind_doc])*
        #[derive(Clone, Copy, Debug, Display, Eq, PartialEq)]
        pub enum $kind_ty {
            $(

                $(#[doc = $inner_ty_doc])*
                $inner_ty,
            )*
        }

        $(#[doc = $msg_doc])*
        #[derive(Debug, Deserialize, Serialize)]
        pub enum $msg_ty {
            $(
                $(#[doc = $inner_ty_doc])*
                $inner_ty($inner_ty),
            )*
        }

        impl Message<'_> for $msg_ty {
            type Kind = $kind_ty;

            fn kind(&self) -> Self::Kind {
                match self {
                    $(
                        $msg_ty::$inner_ty(..) => $kind_ty::$inner_ty,
                    )*
                }
            }
        }

        $(
            impl_message_inner! {
                $(#[doc = $inner_ty_doc])*
                $inner_ty $({
                    $(
                        $(#[doc = $field_doc])*
                        $field: $field_ty,
                    )*
                })?
            }

            impl From<$inner_ty> for $msg_ty {
                fn from(m: $inner_ty) -> Self {
                    $msg_ty::$inner_ty(m)
                }
            }

            impl TryFrom<$msg_ty> for $inner_ty {
                type Error = KindMismatch<$kind_ty>;

                fn try_from(msg: $msg_ty) -> Result<Self, Self::Error> {
                    #[allow(irrefutable_let_patterns)]
                    if let $msg_ty::$inner_ty(msg) = msg {
                        Ok(msg)
                    } else {
                        Err(KindMismatch {
                            expected: $kind_ty::$inner_ty,
                            actual: msg.kind(),
                        })
                    }
                }
            }

            impl MessageContent<'_, $msg_ty, $kind_ty> for $inner_ty {
                fn kind() -> $kind_ty {
                    $kind_ty::$inner_ty
                }
            }
        )*
    };
}

/// A request from the recorder to the runner.
#[derive(Debug, Deserialize, Serialize)]
pub enum RecorderRequest {
    /// A new request.
    ///
    /// If successful, the runner will restart and the recorder should send a
    /// [`ResumeRequest`](enum.RecorderRequest.html#variant.ResumeRequest)
    /// upon reconnection.
    NewRequest(NewRequest),

    /// A request to resume a [previous
    /// request](enum.RecorderRequest.html#variant.NewRequest).
    ResumeRequest(ResumeRequest),
}

impl From<NewRequest> for Request {
    fn from(req: NewRequest) -> Request {
        Request {
            request: RecorderRequest::NewRequest(req),
        }
    }
}

impl From<ResumeRequest> for Request {
    fn from(req: ResumeRequest) -> Request {
        Request {
            request: RecorderRequest::ResumeRequest(req),
        }
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

#[derive(Debug, Deserialize, Serialize)]
pub struct NewRequest {
    /// The task ID of the Taskcluster build task.
    ///
    /// The build artifact from this task will be downloaded by the runner.
    pub build_task_id: String,

    /// The size of the profile that will be sent, if any.
    pub profile_size: Option<u64>,

    /// Prefs to override in the profile.
    pub prefs: Vec<(String, PrefValue)>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ResumeRequest {
    /// Whether or not the runner should wait for idle before running Firefox.
    pub idle: Idle,
}

impl_message! {
    /// A message from FxRecorder to FxRunner.
    RecorderMessage,

    /// The kind of a [`RecorderMessage`](struct.RecorderMessage.html).
    RecorderMessageKind;

    /// A request from the recorder to the runner.
    Request {
        request: RecorderRequest,
    };
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

impl_message! {
    /// A message from FxRunner to FxRecorder.
    RunnerMessage,

    /// The kind of a [`RunnerMessage`](struct.RunnerMessage.html).
    RunnerMessageKind;

    /// The status of the DownloadBuild phase.
    DownloadBuild {
        result: ForeignResult<DownloadStatus>,
    };

    /// The status of the RecvProfile phase.
    RecvProfile {
        result: ForeignResult<DownloadStatus>,
    };

    /// The result of the CreateProfile phase.
    CreateProfile {
        result: ForeignResult<()>,
    };

    /// The status of the WritePrefs phase.
    WritePrefs {
        result: ForeignResult<()>,
    };

    /// The status of the Restarting phase.
    Restarting {
        result: ForeignResult<()>,
    };

    /// The status of the ResumeResponse phase.
    ResumeResponse {
        result: ForeignResult<()>,
    };

    /// The status of the WaitForIdle phase.
    WaitForIdle {
        result: ForeignResult<()>,
    };
}
