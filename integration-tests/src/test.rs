// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::convert::TryInto;
use std::env::current_dir;
use std::future::Future;

use assert_matches::assert_matches;
use futures::join;
use indoc::indoc;
use libfxrecord::error::ErrorMessage;
use libfxrecord::net::*;
use libfxrecorder::proto::{RecorderProto, RecorderProtoError};
use libfxrunner::osapi::ShutdownProvider;
use libfxrunner::proto::{RunnerProto, RunnerProtoError};
use libfxrunner::taskcluster::{Taskcluster, TaskclusterError};
use reqwest::StatusCode;
use serde_json::Value;
use slog::Logger;
use tempfile::TempDir;
use tokio::fs::{create_dir_all, File};
use tokio::net::{TcpListener, TcpStream};
use tokio::prelude::*;
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

/// Discard exactly `size` bytes from the reader.
async fn discard_exact<R>(r: R, size: u64)
where
    R: AsyncRead + Unpin,
{
    tokio::io::copy(&mut r.take(size), &mut tokio::io::sink())
        .await
        .unwrap();
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
                    RecorderProtoError::Proto(ProtoError::Io(..)) => {}
                    RecorderProtoError::Proto(ProtoError::EndOfStream) => {}
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
                RecorderProtoError::Proto(ProtoError::EndOfStream)
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
        |mut recorder| async move {
            assert_matches!(
                recorder.handshake(true).await.unwrap_err(),
                RecorderProtoError::Proto(ProtoError::Foreign(e)) => {
                    assert_eq!(e.to_string(), "could not shutdown");
                }
            );
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
                    RecorderProtoError::Proto(ProtoError::Foreign(ref e)) => {
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
                        RecorderProtoError::Proto(ProtoError::Foreign(e)) => {
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

#[tokio::test]
async fn test_send_profile() {
    let mut listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let test_dir = current_dir().unwrap().parent().unwrap().join("test");

    let profile_zip_path = test_dir.join("profile.zip");
    let nested_profile_zip_path = test_dir.join("profile_nested.zip");

    {
        let tempdir = TempDir::new().unwrap();
        run_proto_test(
            &mut listener,
            TestShutdownProvider::default(),
            |mut runner| {
                let temp_path = tempdir.path();
                async move {
                    assert!(runner
                        .send_profile_reply(temp_path)
                        .await
                        .unwrap()
                        .is_none());
                }
            },
            |mut recorder| async move {
                recorder.send_profile(None).await.unwrap();
            },
        )
        .await;

        assert!(!tempdir.path().join("profile.zip").exists());
        assert!(!tempdir.path().join("profile").exists());
    }

    {
        let tempdir = TempDir::new().unwrap();
        run_proto_test(
            &mut listener,
            TestShutdownProvider::default(),
            |mut runner| {
                let temp_path = tempdir.path();
                async move {
                    let profile_path = runner.send_profile_reply(temp_path).await.unwrap().unwrap();

                    assert_eq!(profile_path, temp_path.join("profile"));
                }
            },
            |mut recorder| {
                let zip_path = &profile_zip_path;
                async move {
                    recorder.send_profile(Some(zip_path)).await.unwrap();
                }
            },
        )
        .await;

        assert!(tempdir.path().join("profile.zip").exists());

        let profile_path = tempdir.path().join("profile");

        assert!(profile_path.is_dir());
        assert!(profile_path.join("places.sqlite").is_file());
        assert!(profile_path.join("prefs.js").is_file());
        assert!(profile_path.join("user.js").is_file());
    }

    {
        let tempdir = TempDir::new().unwrap();
        run_proto_test(
            &mut listener,
            TestShutdownProvider::default(),
            |mut runner| {
                let temp_path = tempdir.path();
                async move {
                    let profile_path = runner.send_profile_reply(temp_path).await.unwrap().unwrap();

                    assert_eq!(profile_path, temp_path.join("profile").join("profile"));

                    assert!(profile_path.join("places.sqlite").is_file());
                    assert!(profile_path.join("prefs.js").is_file());
                    assert!(profile_path.join("user.js").is_file());
                }
            },
            |mut recorder| {
                let zip_path = &nested_profile_zip_path;
                async move {
                    recorder.send_profile(Some(zip_path)).await.unwrap();
                }
            },
        )
        .await;

        assert!(tempdir.path().join("profile.zip").exists());

        let profile_path = tempdir.path().join("profile").join("profile");

        assert!(profile_path.is_dir());
        assert!(profile_path.join("places.sqlite").is_file());
        assert!(profile_path.join("prefs.js").is_file());
        assert!(profile_path.join("user.js").is_file());
    }

    // Testing invalid runner reply when not sending a profile.
    {
        run_proto_test(
            &mut listener,
            TestShutdownProvider::default(),
            |runner| {
                let mut runner = runner.into_inner();

                async move {
                    assert!(runner
                        .recv::<SendProfile>()
                        .await
                        .unwrap()
                        .profile_size
                        .is_none());

                    runner
                        .send(SendProfileReply {
                            result: Ok(Some(DownloadStatus::Downloading)),
                        })
                        .await
                        .unwrap();
                }
            },
            |mut recorder| async move {
                assert_matches!(
                    recorder.send_profile(None).await.unwrap_err(),
                    RecorderProtoError::SendProfileMismatch {
                        expected: None,
                        received: Some(DownloadStatus::Downloading),
                    }
                );
            },
        )
        .await;
    }

    // Testing invalid runner reply when `DownloadStatus::Downloading` was expected.
    {
        run_proto_test(
            &mut listener,
            TestShutdownProvider::default(),
            |runner| {
                let mut runner = runner.into_inner();
                async move {
                    runner
                        .recv::<SendProfile>()
                        .await
                        .unwrap()
                        .profile_size
                        .unwrap();

                    runner
                        .send(SendProfileReply {
                            result: Ok(Some(DownloadStatus::Extracted)),
                        })
                        .await
                        .unwrap();
                }
            },
            |mut recorder| {
                let zip_path = &profile_zip_path;
                async move {
                    assert_matches!(
                        recorder.send_profile(Some(zip_path)).await.unwrap_err(),
                        RecorderProtoError::SendProfileMismatch {
                            expected: Some(DownloadStatus::Downloading),
                            received: Some(DownloadStatus::Extracted),
                        }
                    );
                }
            },
        )
        .await;
    }

    // Testing invalid runner reply when `DownloadStatus::Downloaded` was expected.
    {
        run_proto_test(
            &mut listener,
            TestShutdownProvider::default(),
            |runner| {
                let mut runner = runner.into_inner();

                async move {
                    let size = runner
                        .recv::<SendProfile>()
                        .await
                        .unwrap()
                        .profile_size
                        .unwrap();

                    runner
                        .send(SendProfileReply {
                            result: Ok(Some(DownloadStatus::Downloading)),
                        })
                        .await
                        .unwrap();

                    let mut stream = runner.into_inner();

                    discard_exact(&mut stream, size).await;

                    let mut runner = Proto::<
                        RecorderMessage,
                        RunnerMessage,
                        RecorderMessageKind,
                        RunnerMessageKind,
                    >::new(stream);

                    runner
                        .send(SendProfileReply {
                            result: Ok(Some(DownloadStatus::Extracted)),
                        })
                        .await
                        .unwrap();
                }
            },
            |mut recorder| {
                let zip_path = &profile_zip_path;
                async move {
                    assert_matches!(
                        recorder.send_profile(Some(zip_path)).await.unwrap_err(),
                        RecorderProtoError::SendProfileMismatch {
                            expected: Some(DownloadStatus::Downloaded),
                            received: Some(DownloadStatus::Extracted),
                        }
                    );
                }
            },
        )
        .await;
    }

    // Testing invalid runner reply when `DownloadStatus::Downloaded` was expected.
    {
        run_proto_test(
            &mut listener,
            TestShutdownProvider::default(),
            |runner| {
                let mut runner = runner.into_inner();

                async move {
                    let size = runner
                        .recv::<SendProfile>()
                        .await
                        .unwrap()
                        .profile_size
                        .unwrap();

                    runner
                        .send(SendProfileReply {
                            result: Ok(Some(DownloadStatus::Downloading)),
                        })
                        .await
                        .unwrap();

                    let mut stream = runner.into_inner();

                    discard_exact(&mut stream, size).await;

                    let mut runner = Proto::<
                        RecorderMessage,
                        RunnerMessage,
                        RecorderMessageKind,
                        RunnerMessageKind,
                    >::new(stream);

                    runner
                        .send(SendProfileReply {
                            result: Ok(Some(DownloadStatus::Downloaded)),
                        })
                        .await
                        .unwrap();

                    runner
                        .send(SendProfileReply {
                            result: Ok(Some(DownloadStatus::Downloaded)),
                        })
                        .await
                        .unwrap();
                }
            },
            |mut recorder| {
                let zip_path = &profile_zip_path;
                async move {
                    assert_matches!(
                        recorder.send_profile(Some(zip_path)).await.unwrap_err(),
                        RecorderProtoError::SendProfileMismatch {
                            expected: Some(DownloadStatus::Extracted),
                            received: Some(DownloadStatus::Downloaded),
                        }
                    );
                }
            },
        )
        .await;
    }
}

#[tokio::test]
async fn test_send_prefs() {
    let mut listener = TcpListener::bind("127.0.0.1:0").await.unwrap();

    // Test not sending any prefs.
    {
        let tempdir = TempDir::new().unwrap();
        let profile_path = tempdir.path().join("profile");

        create_dir_all(&profile_path).await.unwrap();
        let prefs_path = profile_path.join("user.js");

        run_proto_test(
            &mut listener,
            TestShutdownProvider::default(),
            |mut runner| {
                let prefs_path = prefs_path.clone();
                async move {
                    runner.send_prefs_reply(&prefs_path).await.unwrap();
                }
            },
            |mut recorder| async move {
                recorder.send_prefs(vec![]).await.unwrap();
            },
        )
        .await;

        assert!(!prefs_path.exists());
    }

    // Test sending a list of prefs.
    {
        let tempdir = TempDir::new().unwrap();
        let profile_path = tempdir.path().join("profile");
        create_dir_all(&profile_path).await.unwrap();
        let prefs_path = profile_path.join("user.js");

        run_proto_test(
            &mut listener,
            TestShutdownProvider::default(),
            |mut runner| {
                let prefs_path = prefs_path.clone();
                async move {
                    runner.send_prefs_reply(&prefs_path).await.unwrap();
                }
            },
            |mut recorder| async move {
                recorder
                    .send_prefs(vec![
                        (
                            "foo".into(),
                            Value::String("bar".into()).try_into().unwrap(),
                        ),
                        ("bar".into(), Value::Bool(true).try_into().unwrap()),
                        ("baz".into(), Value::Number(1i64.into()).try_into().unwrap()),
                    ])
                    .await
                    .unwrap();
            },
        )
        .await;

        assert!(prefs_path.exists());

        let contents = {
            let mut buf = String::new();
            let mut f = File::open(&prefs_path).await.unwrap();

            f.read_to_string(&mut buf).await.unwrap();
            buf
        };

        assert_eq!(
            contents,
            indoc!(
                r#"pref("foo", "bar");
                pref("bar", true);
                pref("baz", 1);
                "#
            )
        );
    }

    // Test appending to an already existing user.js.
    {
        let tempdir = TempDir::new().unwrap();
        let profile_path = tempdir.path().join("profile");

        create_dir_all(&profile_path).await.unwrap();
        let prefs_path = profile_path.join("user.js");

        {
            let mut f = File::create(&prefs_path).await.unwrap();
            f.write_all(
                indoc!(
                    r#"// user.js
                    pref("foo", "bar");
                    // end user.js
                    "#
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        }

        run_proto_test(
            &mut listener,
            TestShutdownProvider::default(),
            |mut runner| {
                let prefs_path = prefs_path.clone();
                async move {
                    runner.send_prefs_reply(&prefs_path).await.unwrap();
                }
            },
            |mut recorder| async move {
                recorder
                    .send_prefs(vec![(
                        "baz".into(),
                        Value::String("qux".into()).try_into().unwrap(),
                    )])
                    .await
                    .unwrap()
            },
        )
        .await;

        let mut buf = String::new();
        let mut f = File::open(&prefs_path).await.unwrap();
        f.read_to_string(&mut buf).await.unwrap();

        assert_eq!(
            buf,
            indoc!(
                r#"// user.js
                pref("foo", "bar");
                // end user.js
                pref("baz", "qux");
                "#
            )
        );
    }
}
