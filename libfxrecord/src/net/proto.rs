// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::fmt::{Debug, Display};
use std::io;

use futures::prelude::*;
use thiserror::Error;
use tokio::net::TcpStream;
use tokio_serde::formats::Json;
use tokio_util::codec::LengthDelimitedCodec;

use crate::error::ErrorMessage;
use crate::net::message::{KindMismatch, Message, MessageContent};

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
    stream: tokio_serde::Framed<
        tokio_util::codec::Framed<TcpStream, LengthDelimitedCodec>,
        R,
        S,
        Json<R, S>,
    >,

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
            stream: tokio_serde::Framed::new(
                tokio_util::codec::Framed::new(stream, LengthDelimitedCodec::new()),
                Json::default(),
            ),
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
        let actual = msg.kind();

        if M::kind() != actual {
            return Err(ProtoError::Unexpected(KindMismatch {
                expected: M::kind(),
                actual,
            }));
        }

        // We know that `M::kind() == msg.kind()` and this is true if and only
        // if `msg` matches the enum variant for the type `M`.
        Ok(M::try_from(msg).expect("M::kind() and msg.kind() are equal"))
    }

    /// Consume the `Proto`, returning the underlying stream.
    pub fn into_inner(self) -> TcpStream {
        self.stream.into_inner().into_inner()
    }
}

/// An error in the protocol.
#[derive(Debug, Error)]
pub enum ProtoError<K: Debug + Display> {
    /// An IO error occurred.
    #[error("IO error: {}", .0)]
    Io(#[from] io::Error),

    /// An error occurred on the remote side of the protocol.
    ///
    /// Due to the error being serialized across the protocol, the underlying
    /// error cannot have a cause.
    #[error("a remote error occurred: {}", .0)]
    Foreign(#[from] ErrorMessage<String>),

    /// The stream was closed unexpectedly.
    #[error("unexpected end of stream")]
    EndOfStream,

    /// An unexpected message type arrived.
    #[error(
        "expected message of kind `{}' but received message of kind `{}'",
        .0.expected,
        .0.actual
    )]
    Unexpected(KindMismatch<K>),
}
