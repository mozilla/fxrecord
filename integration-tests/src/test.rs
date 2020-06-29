// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

mod mocks;
mod util;

use std::convert::TryInto;
use std::future::Future;
use std::path::PathBuf;

use assert_matches::assert_matches;
use futures::join;
use indoc::indoc;
use libfxrecord::net::*;
use libfxrecorder::proto::{RecorderProto, RecorderProtoError};
use libfxrunner::osapi::WaitForIdleError;
use libfxrunner::proto::{RunnerProto, RunnerProtoError};
use libfxrunner::zip::ZipError;
use serde_json::Value;
use slog::Logger;
use tempfile::TempDir;
use tokio::net::{TcpListener, TcpStream};

use crate::mocks::*;
use crate::util::*;

/// Generate a logger for testing.
///
/// The generated logger discards all messages.
fn test_logger() -> Logger {
    Logger::root(slog::Discard, slog::o! {})
}

type TestRunnerProto = RunnerProto<TestShutdownProvider, TestTaskcluster, TestPerfProvider>;
type TestRunnerProtoError =
    RunnerProtoError<TestShutdownProvider, TestTaskcluster, TestPerfProvider>;

/// Run a test with both the recorder and runner protocols.
async fn run_proto_test<Fut>(
    listener: &mut TcpListener,
    shutdown_provider: TestShutdownProvider,
    tc: TestTaskcluster,
    perf_provider: TestPerfProvider,
    recorder_fn: impl FnOnce(RecorderProto) -> Fut,
    runner_fn: impl FnOnce(Result<bool, TestRunnerProtoError>, PathBuf),
) where
    Fut: Future<Output = ()>,
{
    let addr = listener.local_addr().unwrap();

    let runner = async {
        let (stream, _) = listener.accept().await.unwrap();
        let tempdir = TempDir::new().unwrap();

        let result = TestRunnerProto::handle_request(
            test_logger(),
            stream,
            shutdown_provider,
            tc,
            perf_provider,
            tempdir.path(),
        )
        .await;

        runner_fn(result, tempdir.path().to_path_buf());
    };

    let recorder = async {
        let stream = TcpStream::connect(&addr).await.unwrap();
        let proto = RecorderProto::new(test_logger(), stream);

        recorder_fn(proto).await;
    };

    join!(runner, recorder);
}

#[tokio::test]
async fn test_new_request_ok() {
    let mut listener = TcpListener::bind("127.0.0.1:0").await.unwrap();

    run_proto_test(
        &mut listener,
        TestShutdownProvider::default(),
        TestTaskcluster::default(),
        TestPerfProvider::default(),
        |mut recorder| async move {
            recorder
                .send_new_request("task_id", None, vec![])
                .await
                .unwrap();
        },
        |result, working_dir| {
            assert_eq!(result.unwrap(), true);

            assert!(working_dir.join("firefox").join("firefox.exe").is_file());

            let profile_dir = working_dir.join("profile");
            assert!(profile_dir.is_dir());
            assert!(directory_is_empty(&profile_dir));
        },
    )
    .await;

    run_proto_test(
        &mut listener,
        TestShutdownProvider::default(),
        TestTaskcluster::default(),
        TestPerfProvider::default(),
        |mut recorder| async move {
            recorder
                .send_new_request("task_id", Some(&test_dir().join("profile.zip")), vec![])
                .await
                .unwrap();
        },
        |result, working_dir| {
            assert_eq!(result.unwrap(), true);

            assert!(working_dir.join("firefox").join("firefox.exe").is_file());

            let profile_dir = working_dir.join("profile");
            assert_populated_profile(&profile_dir);
            assert_file_contents_eq(&profile_dir.join("user.js"), "");
        },
    )
    .await;

    run_proto_test(
        &mut listener,
        TestShutdownProvider::default(),
        TestTaskcluster::default(),
        TestPerfProvider::default(),
        |mut recorder| async move {
            recorder
                .send_new_request(
                    "task_id",
                    Some(&test_dir().join("profile.zip")),
                    vec![
                        (
                            "foo".into(),
                            Value::String("bar".into()).try_into().unwrap(),
                        ),
                        ("bar".into(), Value::Bool(true).try_into().unwrap()),
                        ("baz".into(), Value::Number(1i64.into()).try_into().unwrap()),
                    ],
                )
                .await
                .unwrap();
        },
        |result, working_dir| {
            assert_eq!(result.unwrap(), true);

            assert!(working_dir.join("firefox").join("firefox.exe").is_file());

            let profile_dir = working_dir.join("profile");
            assert_populated_profile(dbg!(&profile_dir));
            assert_file_contents_eq(
                &profile_dir.join("user.js"),
                indoc!(
                    r#"pref("foo", "bar");
                    pref("bar", true);
                    pref("baz", 1);
                    "#
                ),
            );
        },
    )
    .await;

    run_proto_test(
        &mut listener,
        TestShutdownProvider::default(),
        TestTaskcluster::default(),
        TestPerfProvider::default(),
        |mut recorder| async move {
            recorder
                .send_new_request(
                    "task_id",
                    None,
                    vec![
                        (
                            "foo".into(),
                            Value::String("bar".into()).try_into().unwrap(),
                        ),
                        ("bar".into(), Value::Bool(true).try_into().unwrap()),
                        ("baz".into(), Value::Number(1i64.into()).try_into().unwrap()),
                    ],
                )
                .await
                .unwrap();
        },
        |result, working_dir| {
            assert_eq!(result.unwrap(), true);

            assert!(working_dir.join("firefox").join("firefox.exe").is_file());

            let profile_dir = working_dir.join("profile");
            assert_file_contents_eq(
                &profile_dir.join("user.js"),
                indoc!(
                    r#"pref("foo", "bar");
                    pref("bar", true);
                    pref("baz", 1);
                    "#
                ),
            );
        },
    )
    .await;
}

#[tokio::test]
async fn test_new_request_err_downloadbuild() {
    let mut listener = TcpListener::bind("127.0.0.1:0").await.unwrap();

    run_proto_test(
        &mut listener,
        TestShutdownProvider::default(),
        TestTaskcluster::with_failure(TaskclusterFailureMode::BadZip),
        TestPerfProvider::default(),
        |mut recorder| async move {
            assert_matches!(
                recorder
                    .send_new_request("task_id", None, vec![])
                    .await
                    .unwrap_err(),
                RecorderProtoError::Proto(ProtoError::Foreign(e)) => {
                    assert_eq!(e.to_string(), TestRunnerProtoError::MissingFirefox.to_string());
                }
            );
        },
        |result, working_dir| {
            assert_matches!(result.unwrap_err(), RunnerProtoError::MissingFirefox);

            assert!(!working_dir.join("firefox").exists());
            assert!(!working_dir.join("firefox").join("firefox.exe").exists());
        },
    )
    .await;

    run_proto_test(
        &mut listener,
        TestShutdownProvider::default(),
        TestTaskcluster::with_failure(TaskclusterFailureMode::Generic("404 Not Found")),
        TestPerfProvider::default(),
        |mut recorder| async move {
            assert_matches!(
                recorder
                    .send_new_request("task_id", None, vec![])
                    .await
                    .unwrap_err(),
                RecorderProtoError::Proto(ProtoError::Foreign(e)) => {
                     assert_eq!(e.to_string(), "404 Not Found");
                }
            );
        },
        |result, working_dir| {
            assert_matches!(
                result.unwrap_err(),
                RunnerProtoError::Taskcluster(e) => {
                    assert_eq!(e.to_string(), "404 Not Found");
                }
            );

            assert!(!working_dir.join("firefox").exists());
            assert!(!working_dir.join("firefox").join("firefox.exe").exists());
        },
    )
    .await;

    run_proto_test(
        &mut listener,
        TestShutdownProvider::default(),
        TestTaskcluster::with_failure(TaskclusterFailureMode::NotZip),
        TestPerfProvider::default(),
        |mut recorder| async move {
            assert_matches!(
                recorder
                    .send_new_request("task_id", None, vec![])
                    .await
                    .unwrap_err(),
                RecorderProtoError::Proto(ProtoError::Foreign(e)) => {
                    let msg = e.to_string();
                    assert!(msg.starts_with("could not read zip archive"));
                    assert!(msg.ends_with("Invalid Zip archive: Could not find central directory end"));
                }
            );
        },
        |result, working_dir| {
            assert_matches!(
                result.unwrap_err(),
                RunnerProtoError::Zip(e @ ZipError::ReadArchive{ .. }) => {

                    assert_eq!(
                        e.to_string(),
                        format!(
                            "could not read zip archive `{}': Invalid Zip archive: Could not find central directory end",
                            working_dir.join("firefox.zip").display()
                        )
                    );
                }
            );

            assert!(!working_dir.join("firefox").exists());
            assert!(!working_dir.join("firefox").join("firefox.exe").exists());
        },
    )
    .await;
}

#[tokio::test]
async fn test_new_request_err_recvprofile() {
    let mut listener = TcpListener::bind("127.0.0.1:0").await.unwrap();

    run_proto_test(
        &mut listener,
        TestShutdownProvider::default(),
        TestTaskcluster::default(),
        TestPerfProvider::default(),
        |mut recorder| async move {
            assert_matches!(
                recorder
                    .send_new_request("task_id", Some(&test_dir().join("README.md")), vec![])
                    .await
                    .unwrap_err(),
                RecorderProtoError::Proto(ProtoError::Foreign(e)) => {
                    let msg = e.to_string();
                    assert!(msg.starts_with("could not read zip archive"));
                    assert!(msg.ends_with(
                        "Invalid Zip archive: Could not find central directory end"
                    ));
                }
            );
        },
        |result, working_dir| {
            assert_matches!(
                result.unwrap_err(),
                RunnerProtoError::Zip(e @ ZipError::ReadArchive{ .. }) => {
                    assert_eq!(
                        e.to_string(),
                        format!(
                            "could not read zip archive `{}': Invalid Zip archive: Could not find central directory end",
                            working_dir.join("profile.zip").display()
                        )
                    );
                }
            );

            assert!(working_dir.join("firefox").join("firefox.exe").is_file());
            assert!(!working_dir.join("profile").is_dir());
        },
    )
    .await;

    run_proto_test(
        &mut listener,
        TestShutdownProvider::default(),
        TestTaskcluster::default(),
        TestPerfProvider::default(),
        |mut recorder| async move {
            assert_matches!(
                recorder
                    .send_new_request("task_id", Some(&test_dir().join("empty.zip")), vec![])
                    .await
                    .unwrap_err(),
                RecorderProtoError::Proto(ProtoError::Foreign(e)) => {
                    assert_eq!(e.to_string(), "An empty profile was received");
                }
            );
        },
        |result, working_dir| {
            assert_matches!(result.unwrap_err(), RunnerProtoError::EmptyProfile);

            assert!(working_dir.join("firefox").join("firefox.exe").is_file());
            assert!(!working_dir.join("profile").exists());
        },
    )
    .await;
}

#[tokio::test]
async fn test_new_request_err_restarting() {
    let mut listener = TcpListener::bind("127.0.0.1:0").await.unwrap();

    run_proto_test(
        &mut listener,
        TestShutdownProvider::with_error("could not shut down"),
        TestTaskcluster::default(),
        TestPerfProvider::default(),
        |mut recorder| async move {
            assert_matches!(
                recorder.send_new_request("task_id", None, vec![])
                    .await
                    .unwrap_err(),
                RecorderProtoError::Proto(ProtoError::Foreign(e)) => {
                    assert_eq!(e.to_string(), "could not shut down");
                }
            );
        },
        |result, working_dir| {
            assert_matches!(
                result.unwrap_err(),
                RunnerProtoError::Shutdown(e) => {
                    assert_eq!(e.to_string(), "could not shut down")
                }
            );

            assert!(working_dir.join("firefox").join("firefox.exe").is_file());
            assert!(directory_is_empty(&working_dir.join("profile")));
        },
    )
    .await;
}

#[tokio::test]
async fn test_resume_request_ok() {
    let mut listener = TcpListener::bind("127.0.0.1:0").await.unwrap();

    run_proto_test(
        &mut listener,
        TestShutdownProvider::default(),
        TestTaskcluster::default(),
        TestPerfProvider::default(),
        |mut recorder| async move {
            recorder.send_resume_request(true).await.unwrap();
        },
        |result, _working_dir| {
            assert_eq!(result.unwrap(), false);
        },
    )
    .await;

    run_proto_test(
        &mut listener,
        TestShutdownProvider::default(),
        TestTaskcluster::default(),
        TestPerfProvider::default(),
        |mut recorder| async move {
            recorder.send_resume_request(false).await.unwrap();
        },
        |result, _working_dir| {
            assert_eq!(result.unwrap(), false);
        },
    )
    .await;
}

#[tokio::test]
async fn test_resume_request_err_waitforidle() {
    let mut listener = TcpListener::bind("127.0.0.1:0").await.unwrap();

    run_proto_test(
        &mut listener,
        TestShutdownProvider::default(),
        TestTaskcluster::default(),
        TestPerfProvider::with_failure(PerfFailureMode::DiskIoError("disk io error")),
        |mut recorder| async move {
            assert_matches!(
                recorder
                    .send_resume_request(true)
                    .await
                    .unwrap_err(),
                RecorderProtoError::Proto(ProtoError::Foreign(e)) => {
                    assert_eq!(
                        e.to_string(),
                        "disk io error"
                    );
                }
            );
        },
        |result, _working_dir| {
            assert_matches!(
                result.unwrap_err(),
                RunnerProtoError::WaitForIdle(WaitForIdleError::DiskIoError(e)) => {
                    assert_eq!(e.to_string(), "disk io error");
                }
            );
        },
    )
    .await;

    run_proto_test(
        &mut listener,
        TestShutdownProvider::default(),
        TestTaskcluster::default(),
        TestPerfProvider::with_failure(PerfFailureMode::CpuTimeError("cpu time error")),
        |mut recorder| async move {
            assert_matches!(
                recorder
                    .send_resume_request(true)
                    .await
                    .unwrap_err(),
                RecorderProtoError::Proto(ProtoError::Foreign(e)) => {
                    assert_eq!(
                        e.to_string(),
                        "cpu time error"
                    );
                }
            );
        },
        |result, _working_dir| {
            assert_matches!(
                result.unwrap_err(),
                RunnerProtoError::WaitForIdle(WaitForIdleError::CpuTimeError(e)) => {
                    assert_eq!(e.to_string(), "cpu time error");
                }
            );
        },
    )
    .await;

    run_proto_test(
        &mut listener,
        TestShutdownProvider::default(),
        TestTaskcluster::default(),
        TestPerfProvider::with_failure(PerfFailureMode::DiskNeverIdle),
        |mut recorder| async move {
            assert_matches!(
                recorder
                    .send_resume_request(true)
                    .await
                    .unwrap_err(),
                RecorderProtoError::Proto(ProtoError::Foreign(e)) => {
                    assert_eq!(
                        e.to_string(),
                        "timed out waiting for CPU and disk to become idle"
                    );
                }
            );
        },
        |result, _working_dir| {
            assert_matches!(
                result.unwrap_err(),
                RunnerProtoError::WaitForIdle(WaitForIdleError::TimeoutError)
            );
        },
    )
    .await;

    run_proto_test(
        &mut listener,
        TestShutdownProvider::default(),
        TestTaskcluster::default(),
        TestPerfProvider::with_failure(PerfFailureMode::CpuNeverIdle),
        |mut recorder| async move {
            assert_matches!(
                recorder
                    .send_resume_request(true)
                    .await
                    .unwrap_err(),
                RecorderProtoError::Proto(ProtoError::Foreign(e)) => {
                    assert_eq!(
                        e.to_string(),
                        "timed out waiting for CPU and disk to become idle"
                    );
                }
            );
        },
        |result, _working_dir| {
            assert_matches!(
                result.unwrap_err(),
                RunnerProtoError::WaitForIdle(WaitForIdleError::TimeoutError)
            );
        },
    )
    .await;
}
