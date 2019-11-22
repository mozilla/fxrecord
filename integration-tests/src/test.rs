// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::env::current_dir;
use std::future::Future;

use assert_matches::assert_matches;
use futures::join;
use libfxrecord::error::ErrorMessage;
use libfxrecord::net::*;
use libfxrecorder::proto::RecorderProto;
use libfxrunner::proto::{RunnerProto, RunnerProtoError};
use libfxrunner::shutdown::ShutdownProvider;
use libfxrunner::taskcluster::{Taskcluster, TaskclusterError};
use reqwest::StatusCode;
use slog::Logger;
use tempfile::TempDir;
use tokio::net::{TcpListener, TcpStream};
use url::Url;

#[derive(Default)]
pub struct TestShutdownProvider {
    error: Option<&'static str>,
}

impl TestShutdownProvider {
    pub fn with_error(s: &'static str) -> Self {
        TestShutdownProvider { error: Some(s) }
    }
}

impl ShutdownProvider for TestShutdownProvider {
    type Error = ErrorMessage<&'static str>;

    fn initiate_restart(&self, _reason: &str) -> Result<(), Self::Error> {
        match self.error {
            Some(ref e) => Err(ErrorMessage(e)),
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

/// Generate a Taskcluster instance that points at mockito.
fn test_tc() -> Taskcluster {
    Taskcluster::with_queue_url(
        Url::parse(&mockito::server_url())
            .unwrap()
            .join("/api/queue/v1/")
            .unwrap(),
    )
}

/// Run a test with both the recorder and runner protocols.
async fn run_proto_test<T, U>(
    listener: &mut TcpListener,
    shutdown: TestShutdownProvider,
    runner_fn: impl FnOnce(RunnerProto<TestShutdownProvider>) -> T,
    recorder_fn: impl FnOnce(RecorderProto) -> U,
) where
    T: Future<Output = ()>,
    U: Future<Output = ()>,
{
    let addr = listener.local_addr().unwrap();

    let runner = async {
        let (stream, _) = listener.accept().await.unwrap();
        let proto = RunnerProto::new(test_logger(), stream, shutdown, test_tc());

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
    let mut listener = TcpListener::bind("127.0.0.1:0").await.unwrap();

    // Test runner dropping connection before receiving handshake.
    run_proto_test(
        &mut listener,
        TestShutdownProvider::default(),
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
        TestShutdownProvider::default(),
        |mut runner| async move {
            assert_matches!(
                runner.handshake_reply().await.unwrap_err(),
                RunnerProtoError::Proto(ProtoError::EndOfStream)
            );
        },
        |_| async move {},
    )
    .await;

    // Test runner dropping connection before end of handshake.
    run_proto_test(
        &mut listener,
        TestShutdownProvider::default(),
        |runner| async move {
            runner.into_inner().recv::<Handshake>().await.unwrap();
        },
        |mut recorder| async move {
            assert_matches!(
                recorder.handshake(true).await.unwrap_err(),
                ProtoError::EndOfStream
            );
        },
    )
    .await;

    // Test handshake protocol.
    run_proto_test(
        &mut listener,
        TestShutdownProvider::default(),
        |mut runner| async move {
            assert!(runner.handshake_reply().await.unwrap());
        },
        |mut recorder| async move {
            recorder.handshake(true).await.unwrap();
        },
    )
    .await;

    // Test handshake protocol with false.
    run_proto_test(
        &mut listener,
        TestShutdownProvider::default(),
        |mut runner| async move {
            assert!(!runner.handshake_reply().await.unwrap());
        },
        |mut recorder| async move {
            recorder.handshake(false).await.unwrap();
        },
    )
    .await;

    // Test handshake protocol with failed shutdown.
    run_proto_test(
        &mut listener,
        TestShutdownProvider::with_error("could not shutdown"),
        |mut runner| async move {
            assert_matches!(runner.handshake_reply().await.unwrap_err(),
                RunnerProtoError::Shutdown(e) => {
                    assert_eq!(e.to_string(), "could not shutdown");
                }
            );
        },
        |mut recorder| {
            use libfxrecorder::proto::ProtoError;
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

#[tokio::test]
async fn test_download_build() {
    let mut listener = TcpListener::bind("127.0.0.1:0").await.unwrap();

    {
        let download_dir = TempDir::new().unwrap();
        let zip_path = current_dir()
            .unwrap()
            .parent()
            .unwrap()
            .join("test")
            .join("firefox.zip");

        let artifact_rsp = mockito::mock(
            "GET",
            "/api/queue/v1/task/foo/artifacts/public/build/target.zip",
        )
        .with_body_from_file(&zip_path)
        .create();

        run_proto_test(
            &mut listener,
            TestShutdownProvider::default(),
            |mut runner| async move {
                runner
                    .download_build_reply(download_dir.path())
                    .await
                    .unwrap();
            },
            |mut recorder| async move {
                recorder.download_build("foo").await.unwrap();
            },
        )
        .await;

        artifact_rsp.assert();
    }

    {
        let download_dir = TempDir::new().unwrap();
        let artifact_rsp = mockito::mock(
            "GET",
            "/api/queue/v1/task/foo/artifacts/public/build/target.zip",
        )
        .with_status(404)
        .with_body("not found")
        .create();

        run_proto_test(
            &mut listener,
            TestShutdownProvider::default(),
            |mut runner| async move {
                assert_matches!(
                    runner
                        .download_build_reply(download_dir.path())
                        .await
                        .unwrap_err(),
                    RunnerProtoError::Taskcluster(TaskclusterError::StatusError(
                        StatusCode::NOT_FOUND
                    ))
                );
            },
            |mut recorder| async move {
                assert_matches!(
                    recorder.download_build("foo").await.unwrap_err(),
                    ProtoError::Foreign(ref e) => {
                        assert_eq!(
                            e.to_string(),
                            "an error occurred while downloading the artifact: 404 Not Found"
                        )
                    }
                );
            },
        )
        .await;

        artifact_rsp.assert();
    }

    {
        let download_dir = TempDir::new().unwrap();
        let zip_path = current_dir()
            .unwrap()
            .parent()
            .unwrap()
            .join("test")
            .join("test.zip");

        let artifact_rsp = mockito::mock(
            "GET",
            "/api/queue/v1/task/foo/artifacts/public/build/target.zip",
        )
        .with_body_from_file(&zip_path)
        .create();

        run_proto_test(
            &mut listener,
            TestShutdownProvider::default(),
            |mut runner| {
                async move {
                    assert_matches!(
                        runner
                            .download_build_reply(download_dir.path())
                            .await
                            .unwrap_err(),
                        RunnerProtoError::MissingFirefox
                    );
                }
            },
            |mut recorder| {
                async move {
                    assert_matches!(
                        recorder.download_build("foo").await.unwrap_err(),
                        ProtoError::Foreign(e) => {
                            assert_eq!(
                                e.to_string(),
                                RunnerProtoError::<<TestShutdownProvider as ShutdownProvider>::Error>::MissingFirefox.to_string()
                            );
                        }
                    );
                }
            },
        )
        .await;

        artifact_rsp.assert();
    }
}
