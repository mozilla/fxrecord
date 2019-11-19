// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::future::Future;

use assert_matches::assert_matches;
use futures::join;
use fxrecorder::proto::RecorderProto;
use fxrunner::proto::{HandshakeError, RunnerProto};
use fxrunner::shutdown::Shutdown;
use libfxrecord::error::ErrorMessage;
use libfxrecord::net::*;
use slog::Logger;
use tokio::net::{TcpListener, TcpStream};

#[derive(Default)]
pub struct TestShutdown {
    error: Option<String>,
}

impl TestShutdown {
    pub fn with_error(s: &str) -> Self {
        TestShutdown {
            error: Some(s.into()),
        }
    }
}

impl Shutdown for TestShutdown {
    type Error = ErrorMessage<String>;

    fn initiate_restart(&self, _reason: &str) -> Result<(), Self::Error> {
        match self.error {
            Some(ref e) => Err(ErrorMessage(e.into())),
            None => Ok(()),
        }
    }
}

/// Generate a logger for testing.
///
/// The generated logger discards all messages.
fn test_logger() -> Logger {
    Logger::root(slog::Discard, slog::o! {})
}

/// Run a test with both the recorder and runner protocols.
async fn run_proto_test<T, U>(
    listener: &mut TcpListener,
    shutdown: TestShutdown,
    runner_fn: impl FnOnce(RunnerProto<TestShutdown>) -> T,
    recorder_fn: impl FnOnce(RecorderProto) -> U,
) where
    T: Future<Output = ()>,
    U: Future<Output = ()>,
{
    let addr = listener.local_addr().unwrap();

    let runner = async {
        let (stream, _) = listener.accept().await.unwrap();
        let proto = RunnerProto::new(test_logger(), stream, shutdown);

        runner_fn(proto).await;
    };

    let recorder = async {
        let stream = TcpStream::connect(&addr).await.unwrap();
        let proto = RecorderProto::new(test_logger(), stream);

        recorder_fn(proto).await;
    };

    join!(runner, recorder);
}

#[tokio::test]
async fn test_handshake() {
    let mut listener = TcpListener::bind("127.0.0.1:9999").await.unwrap();

    // Test runner dropping connection before receiving handshake.
    run_proto_test(
        &mut listener,
        TestShutdown::default(),
        |_| async move {},
        |mut recorder| {
            async move {
                // It is non-deterministic which error we will get.
                match recorder.handshake(false).await.unwrap_err() {
                    ProtoError::Io(..) => {}
                    ProtoError::EndOfStream => {}
                    e => panic!("unexpected error: {:?}", e),
                }
            }
        },
    )
    .await;

    // Test recorder dropping connection before handshaking.
    run_proto_test(
        &mut listener,
        TestShutdown::default(),
        |mut runner| {
            async move {
                assert_matches!(
                    runner.handshake_reply().await.unwrap_err(),
                    HandshakeError::Proto(ProtoError::EndOfStream)
                );
            }
        },
        |_| async move {},
    )
    .await;

    // Test runner dropping connection before end of handshake.
    run_proto_test(
        &mut listener,
        TestShutdown::default(),
        |runner| {
            async move {
                runner.into_inner().recv::<Handshake>().await.unwrap();
            }
        },
        |mut recorder| {
            async move {
                assert_matches!(
                    recorder.handshake(true).await.unwrap_err(),
                    ProtoError::EndOfStream
                );
            }
        },
    )
    .await;

    // Test handshake protocol.
    run_proto_test(
        &mut listener,
        TestShutdown::default(),
        |mut runner| {
            async move {
                assert!(runner.handshake_reply().await.unwrap());
            }
        },
        |mut recorder| {
            async move {
                recorder.handshake(true).await.unwrap();
            }
        },
    )
    .await;

    // Test handshake protocol with false.
    run_proto_test(
        &mut listener,
        TestShutdown::default(),
        |mut runner| {
            async move {
                assert!(!runner.handshake_reply().await.unwrap());
            }
        },
        |mut recorder| {
            async move {
                recorder.handshake(false).await.unwrap();
            }
        },
    )
    .await;

    // Test handshake protocol with failed shutdown.
    run_proto_test(
        &mut listener,
        TestShutdown::with_error("could not shutdown"),
        |mut runner| {
            async move {
                assert_matches!(runner.handshake_reply().await.unwrap_err(),
                    HandshakeError::Shutdown(e) => {
                        assert_eq!(e.to_string(), "could not shutdown");
                    }
                );
            }
        },
        |mut recorder| {
            use fxrecorder::proto::ProtoError;
            async move {
                assert_matches!(
                    recorder.handshake(true).await.unwrap_err(),
                    ProtoError::Foreign(e) => {
                        assert_eq!(e.to_string(), "could not shutdown");
                    }
                );
            }
        },
    )
    .await;
}
