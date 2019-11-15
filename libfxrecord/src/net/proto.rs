// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::error::Error;
use std::fmt::{Debug, Display};
use std::io;

use derive_more::Display;
use futures::prelude::*;
use tokio::codec::{Framed, LengthDelimitedCodec};
use tokio::net::TcpStream;
use tokio_serde_json::{ReadJson, WriteJson};

use crate::error::ErrorMessage;
pub use crate::net::message::{Message, MessageContent, RecorderMessage, RunnerMessage};

/// A protocol for receiving messages of type `R` and sending messages of type
/// `S` over a `TcpStream`.
///
/// Messages are JSON-encoded and prefixed with their length before transmission.
///
/// Here `RK` and `SK` are the kinds of the message types `R` and `S`
/// respectively, as per the [`Message`](trait.Message.html#associatedtype.Kind) trait.
pub struct Proto<R, S, RK, SK>
where
    for<'de> R: Message<'de, Kind = RK>,
    for<'de> S: Message<'de, Kind = SK>,
    RK: Debug + Display + Eq + PartialEq,
    SK: Debug + Display + Eq + PartialEq,
{
    stream: ReadJson<WriteJson<Framed<TcpStream, LengthDelimitedCodec>, S>, R>,

    // We need to include `RK` and `SK ` in the type signature for this struct
    // to get around limitations with HKT.
    _marker: std::marker::PhantomData<(RK, SK)>,
}

impl<R, S, RK, SK> Proto<R, S, RK, SK>
where
    for<'de> R: Message<'de, Kind = RK>,
    for<'de> S: Message<'de, Kind = SK>,
    RK: Debug + Display + Eq + PartialEq,
    SK: Debug + Display + Eq + PartialEq,
{
    /// Wrap the stream for communicating via messages.
    pub fn new(stream: TcpStream) -> Self {
        Self {
            stream: ReadJson::new(WriteJson::new(Framed::new(
                stream,
                LengthDelimitedCodec::new(),
            ))),
            _marker: std::marker::PhantomData,
        }
    }

    /// Send a message.
    pub async fn send<M>(&mut self, msg: M) -> Result<(), ProtoError<RK>>
    where
        for<'de> M: MessageContent<'de, S, SK>,
    {
        self.stream.send(msg.into()).await.map_err(Into::into)
    }

    /// Receive a specific message kind.
    ///
    /// Any message returned that is not of the specified kind will cause an error.
    pub async fn recv<M>(&mut self) -> Result<M, ProtoError<RK>>
    where
        for<'de> M: MessageContent<'de, R, RK>,
    {
        let msg = self
            .stream
            .try_next()
            .await?
            .ok_or(ProtoError::EndOfStream)?;
        let received = msg.kind();

        if M::kind() != received {
            return Err(ProtoError::Unexpected {
                expected: M::kind(),
                received,
            });
        }

        // We know that `M::kind() == msg.kind()` and this is true if and only
        // if `msg` matches the enum variant for the type `M`.
        Ok(M::try_from(msg).expect("M::kind() and msg.kind() are equal"))
    }

    /// Consume the `Proto`, returning the underlying stream.
    pub fn into_inner(self) -> TcpStream {
        self.stream.into_inner().into_inner().into_inner()
    }
}

/// An error in the protocol.
#[derive(Debug, Display)]
pub enum ProtoError<K: Debug + Display> {
    /// An IO error occurred.
    #[display(fmt = "IO error: {}", _0)]
    Io(io::Error),

    /// An error occurred on the remote side of the protocol.
    ///
    /// Due to the error being serialized across the protocol, the underlying
    /// error cannot have a cause.
    #[display(fmt = "a remote error occurred: {}", _0)]
    Foreign(ErrorMessage<String>),

    /// The stream was closed unexpectedly.
    #[display(fmt = "unexpected end of stream")]
    EndOfStream,

    /// An unexpected message type arrived.
    #[display(
        fmt = "expected message of kind `{}' but received message of kind `{}'",
        expected,
        received
    )]
    Unexpected {
        /// The type of message that was expected.
        expected: K,
        /// The type of message that was received.
        received: K,
    },
}

impl<K: Debug + Display> Error for ProtoError<K> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            ProtoError::Io(ref e) => Some(e),
            ProtoError::Foreign(ref e) => Some(e),
            ProtoError::EndOfStream => None,
            ProtoError::Unexpected { .. } => None,
        }
    }
}

impl<K: Debug + Display> From<io::Error> for ProtoError<K> {
    fn from(e: io::Error) -> Self {
        ProtoError::Io(e)
    }
}
