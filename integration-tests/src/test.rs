// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

mod mocks;
mod util;

use std::convert::TryInto;
use std::future::Future;

use assert_matches::assert_matches;
use futures::join;
use indoc::indoc;
use libfxrecord::net::*;
use libfxrecorder::proto::{RecorderProto, RecorderProtoError};
use libfxrunner::osapi::WaitForIdleError;
use libfxrunner::proto::{RunnerProto, RunnerProtoError};
use libfxrunner::request::{
    NewRequestError, RequestInfo, ResumeRequestError, ResumeRequestErrorKind,
};
use libfxrunner::zip::ZipError;
use serde_json::Value;
use slog::Logger;
use tokio::net::{TcpListener, TcpStream};

use crate::mocks::*;
use crate::util::*;

/// Generate a logger for testing.
///
/// The generated logger discards all messages.
fn test_logger() -> Logger {
    Logger::root(slog::Discard, slog::o! {})
}

type TestRunnerProto =
    RunnerProto<TestShutdownProvider, TestTaskcluster, TestPerfProvider, TestRequestManager>;
type TestRunnerProtoError =
    RunnerProtoError<TestShutdownProvider, TestTaskcluster, TestPerfProvider>;

struct RunnerInfo {
    result: Result<bool, TestRunnerProtoError>,
    request_info: Option<RequestInfo<'static>>,
}

/// Run a test with both the recorder and runner protocols.
async fn run_proto_test<'a, Fut>(
    listener: &mut TcpListener,
    shutdown_provider: TestShutdownProvider,
    tc: TestTaskcluster,
    perf_provider: TestPerfProvider,
    request_manager: TestRequestManager,
    recorder_fn: impl FnOnce(RecorderProto) -> Fut,
    runner_fn: impl FnOnce(RunnerInfo),
) where
    Fut: Future<Output = ()>,
{
    let addr = listener.local_addr().unwrap();

    let runner = async {
        let (stream, _) = listener.accept().await.unwrap();

        let handle = request_manager.handle();

        let result = TestRunnerProto::handle_request(
            test_logger(),
            stream,
            shutdown_provider,
            tc,
            perf_provider,
            request_manager,
        )
        .await;

        runner_fn(RunnerInfo {
            result,
            request_info: handle.last_request_info(),
        });
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
        TestRequestManager::default(),
        |mut recorder| async move {
            assert_eq!(
                recorder
                    .send_new_request("task_id", None, vec![])
                    .await
                    .unwrap(),
                VALID_REQUEST_ID
            );
        },
        |RunnerInfo {
             result,
             request_info,
         }| {
            assert_eq!(result.unwrap(), true);

            let request_info = request_info.unwrap();
            assert!(request_info
                .path
                .join("firefox")
                .join("firefox.exe")
                .is_file());

            let profile_dir = request_info.path.join("profile");
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
        TestRequestManager::default(),
        |mut recorder| async move {
            assert_eq!(
                recorder
                    .send_new_request("task_id", Some(&test_dir().join("profile.zip")), vec![])
                    .await
                    .unwrap(),
                VALID_REQUEST_ID
            );
        },
        |RunnerInfo {
             result,
             request_info,
         }| {
            assert_eq!(result.unwrap(), true);

            let request_info = request_info.unwrap();
            assert!(request_info
                .path
                .join("firefox")
                .join("firefox.exe")
                .is_file());

            let profile_dir = request_info.path.join("profile");
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
        TestRequestManager::default(),
        |mut recorder| async move {
            let request_id = recorder
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

            assert_eq!(request_id, VALID_REQUEST_ID);
        },
        |RunnerInfo {
             result,
             request_info,
         }| {
            assert_eq!(result.unwrap(), true);

            let request_info = request_info.unwrap();
            assert!(request_info
                .path
                .join("firefox")
                .join("firefox.exe")
                .is_file());

            let profile_dir = request_info.path.join("profile");
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
        TestRequestManager::default(),
        |mut recorder| async move {
            let request_id = recorder
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

            assert_eq!(request_id, VALID_REQUEST_ID);
        },
        |RunnerInfo {
             result,
             request_info,
         }| {
            assert_eq!(result.unwrap(), true);

            let request_info = request_info.unwrap();
            assert!(request_info
                .path
                .join("firefox")
                .join("firefox.exe")
                .is_file());

            let profile_dir = request_info.path.join("profile");
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
async fn test_new_request_err_request_manager() {
    let mut listener = TcpListener::bind("127.0.0.1:0").await.unwrap();

    run_proto_test(
        &mut listener,
        TestShutdownProvider::default(),
        TestTaskcluster::default(),
        TestPerfProvider::default(),
        TestRequestManager::with_failure(RequestFailureMode::NewRequest(
            NewRequestError::TooManyAttempts(32),
        )),
        |mut recorder| async move {
            assert_matches!(
                recorder.send_new_request("task_id", None, vec![]).await.unwrap_err(),
                RecorderProtoError::Proto(ProtoError::Foreign(e)) => {
                    assert_eq!(
                        e.to_string(),
                        "Could not create a request directory after 32 attempts");
                }
            );
        },
        |RunnerInfo {
             result,
             request_info,
         }| {
            assert_matches!(
                result.unwrap_err(),
                RunnerProtoError::NewRequest(NewRequestError::TooManyAttempts(32))
            );

            assert!(request_info.is_none());
        },
    )
    .await;

    run_proto_test(
        &mut listener,
        TestShutdownProvider::default(),
        TestTaskcluster::default(),
        TestPerfProvider::default(),
        TestRequestManager::with_failure(RequestFailureMode::EnsureProfileDir(
            "could not ensure profile directory",
        )),
        |mut recorder| async move {
            assert_matches!(
                recorder.send_new_request("task_id", None, vec![]).await.unwrap_err(),
                RecorderProtoError::Proto(ProtoError::Foreign(e)) => {
                    assert_eq!(
                        e.to_string(),
                        "could not ensure profile directory");
                }
            );
        },
        |RunnerInfo {
             result,
             request_info,
         }| {
            assert_matches!(
                result.unwrap_err(),
                RunnerProtoError::EnsureProfile(e) => {
                    assert_eq!(e.to_string(), "could not ensure profile directory");
                }
            );

            let request_info = request_info.unwrap();
            assert_eq!(request_info.id, VALID_REQUEST_ID);
            assert!(!request_info.path.exists());
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
        TestRequestManager::default(),
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
        |RunnerInfo {
             result,
             request_info,
         }| {
            assert_matches!(result.unwrap_err(), RunnerProtoError::MissingFirefox);

            let request_info = request_info.unwrap();
            assert_eq!(request_info.id, VALID_REQUEST_ID);

            assert!(!request_info.path.exists());
        },
    )
    .await;

    run_proto_test(
        &mut listener,
        TestShutdownProvider::default(),
        TestTaskcluster::with_failure(TaskclusterFailureMode::Generic("404 Not Found")),
        TestPerfProvider::default(),
        TestRequestManager::default(),
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
        |RunnerInfo {
             result,
             request_info,
         }| {
            assert_matches!(
                result.unwrap_err(),
                RunnerProtoError::Taskcluster(e) => {
                    assert_eq!(e.to_string(), "404 Not Found");
                }
            );

            let request_info = request_info.unwrap();
            assert_eq!(request_info.id, VALID_REQUEST_ID);
            assert!(!request_info.path.exists());
        },
    )
    .await;

    run_proto_test(
        &mut listener,
        TestShutdownProvider::default(),
        TestTaskcluster::with_failure(TaskclusterFailureMode::NotZip),
        TestPerfProvider::default(),

        TestRequestManager::default(),
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
        |RunnerInfo { result, request_info }| {
            let request_info = request_info.unwrap();
            assert_eq!(request_info.id, VALID_REQUEST_ID);

            assert_matches!(
                result.unwrap_err(),
                RunnerProtoError::Zip(e @ ZipError::ReadArchive{ .. }) => {

                    assert_eq!(
                        e.to_string(),
                        format!(
                            "could not read zip archive `{}': Invalid Zip archive: Could not find central directory end",
                            request_info.path.join("firefox.zip").display()
                        )
                    );
                }
            );

            assert!(!request_info.path.exists());
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

        TestRequestManager::default(),
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
        |RunnerInfo { result, request_info }| {
            let request_info = request_info.unwrap();
            assert_eq!(request_info.id, VALID_REQUEST_ID);

            assert_matches!(
                result.unwrap_err(),
                RunnerProtoError::Zip(e @ ZipError::ReadArchive{ .. }) => {
                    assert_eq!(
                        e.to_string(),
                        format!(
                            "could not read zip archive `{}': Invalid Zip archive: Could not find central directory end",
                            request_info.path.join("profile.zip").display()
                        )
                    );
                }
            );

            assert!(!request_info.path.exists());
        },
    )
    .await;

    run_proto_test(
        &mut listener,
        TestShutdownProvider::default(),
        TestTaskcluster::default(),
        TestPerfProvider::default(),
        TestRequestManager::default(),
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
        |RunnerInfo {
             result,
             request_info,
         }| {
            assert_matches!(result.unwrap_err(), RunnerProtoError::EmptyProfile);

            let request_info = request_info.unwrap();
            assert!(!request_info.path.exists());
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
        TestRequestManager::default(),
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
        |RunnerInfo {
             result,
             request_info,
         }| {
            assert_matches!(
                result.unwrap_err(),
                RunnerProtoError::Shutdown(e) => {
                    assert_eq!(e.to_string(), "could not shut down")
                }
            );

            let request_info = request_info.unwrap();
            assert!(!request_info.path.exists());
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
        TestPerfProvider::asserting_invoked(),
        TestRequestManager::default(),
        |mut recorder| async move {
            recorder
                .send_resume_request(VALID_REQUEST_ID, Idle::Wait)
                .await
                .unwrap();
        },
        |RunnerInfo {
             result,
             request_info,
         }| {
            assert_eq!(result.unwrap(), false);
            assert_eq!(request_info.unwrap().id, VALID_REQUEST_ID);
        },
    )
    .await;

    run_proto_test(
        &mut listener,
        TestShutdownProvider::default(),
        TestTaskcluster::default(),
        TestPerfProvider::asserting_not_invoked(),
        TestRequestManager::default(),
        |mut recorder| async move {
            recorder
                .send_resume_request(VALID_REQUEST_ID, Idle::Skip)
                .await
                .unwrap();
        },
        |RunnerInfo {
             result,
             request_info,
         }| {
            assert_eq!(result.unwrap(), false);
            assert_eq!(request_info.unwrap().id, VALID_REQUEST_ID);
        },
    )
    .await;
}

#[tokio::test]
async fn test_resume_request_err_request_manager() {
    let mut listener = TcpListener::bind("127.0.0.1:0").await.unwrap();

    run_proto_test(
        &mut listener,
        TestShutdownProvider::default(),
        TestTaskcluster::default(),
        TestPerfProvider::default(),
        TestRequestManager::default(),
        |mut recorder| async move {
            assert_matches!(
                // Any request that is not VALID_REQUEST_ID triggers this error.
                recorder.send_resume_request("foobar", Idle::Skip).await.unwrap_err(),
                RecorderProtoError::Proto(ProtoError::Foreign(e)) => {
                    assert_eq!(e.to_string(), "Invalid request `foobar': ID contains invalid characters");
                }
            );
        },
        |RunnerInfo {
             result,
             request_info,
         }| {
             assert_matches!(
                result.unwrap_err(),
                RunnerProtoError::ResumeRequest(e) => {
                    assert_eq!(e, ResumeRequestError {
                        kind: ResumeRequestErrorKind::InvalidId,
                        request_id: "foobar".into(),
                    });
                }
            );

            assert!(request_info.is_none());
         },
    )
    .await;

    run_proto_test(
        &mut listener,
        TestShutdownProvider::default(),
        TestTaskcluster::default(),
        TestPerfProvider::default(),
        TestRequestManager::with_failure(RequestFailureMode::ResumeRequest(
            ResumeRequestErrorKind::MissingProfile,
        )),
        |mut recorder| async move {
            assert_matches!(
                recorder
                    .send_resume_request(VALID_REQUEST_ID, Idle::Skip)
                    .await
                    .unwrap_err(),
                RecorderProtoError::Proto(ProtoError::Foreign(e)) => {
                    assert_eq!(
                        e.to_string(),
                        "Invalid request `REQUESTID': missing a profile directory"
                    );
                }
            );
        },
        |RunnerInfo {
             result,
             request_info,
         }| {
            assert_matches!(
                result.unwrap_err(),
                RunnerProtoError::ResumeRequest(e) => {
                    assert_eq!(
                        e,
                        ResumeRequestError {
                            kind: ResumeRequestErrorKind::MissingProfile,
                            request_id: VALID_REQUEST_ID.into(),
                        }
                    );
                }
            );

            assert!(request_info.is_none());
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
        TestRequestManager::default(),
        |mut recorder| async move {
            assert_matches!(
                recorder
                    .send_resume_request(VALID_REQUEST_ID, Idle::Wait)
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
        |RunnerInfo {
             result,
             request_info,
         }| {
            assert_matches!(
                result.unwrap_err(),
                RunnerProtoError::WaitForIdle(WaitForIdleError::DiskIoError(e)) => {
                    assert_eq!(e.to_string(), "disk io error");
                }
            );

            assert_eq!(request_info.unwrap().id, VALID_REQUEST_ID);
        },
    )
    .await;

    run_proto_test(
        &mut listener,
        TestShutdownProvider::default(),
        TestTaskcluster::default(),
        TestPerfProvider::with_failure(PerfFailureMode::CpuTimeError("cpu time error")),
        TestRequestManager::default(),
        |mut recorder| async move {
            assert_matches!(
                recorder
                    .send_resume_request(VALID_REQUEST_ID, Idle::Wait)
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
        |RunnerInfo {
             result,
             request_info,
         }| {
            assert_matches!(
                result.unwrap_err(),
                RunnerProtoError::WaitForIdle(WaitForIdleError::CpuTimeError(e)) => {
                    assert_eq!(e.to_string(), "cpu time error");
                }
            );
            assert_eq!(request_info.unwrap().id, VALID_REQUEST_ID);
        },
    )
    .await;

    run_proto_test(
        &mut listener,
        TestShutdownProvider::default(),
        TestTaskcluster::default(),
        TestPerfProvider::with_failure(PerfFailureMode::DiskNeverIdle),
        TestRequestManager::default(),
        |mut recorder| async move {
            assert_matches!(
                recorder
                    .send_resume_request(VALID_REQUEST_ID, Idle::Wait)
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
        |RunnerInfo {
             result,
             request_info,
         }| {
            assert_matches!(
                result.unwrap_err(),
                RunnerProtoError::WaitForIdle(WaitForIdleError::TimeoutError)
            );
            assert_eq!(request_info.unwrap().id, VALID_REQUEST_ID);
        },
    )
    .await;

    run_proto_test(
        &mut listener,
        TestShutdownProvider::default(),
        TestTaskcluster::default(),
        TestPerfProvider::with_failure(PerfFailureMode::CpuNeverIdle),
        TestRequestManager::default(),
        |mut recorder| async move {
            assert_matches!(
                recorder
                    .send_resume_request(VALID_REQUEST_ID, Idle::Wait)
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
        |RunnerInfo {
             result,
             request_info,
         }| {
            assert_matches!(
                result.unwrap_err(),
                RunnerProtoError::WaitForIdle(WaitForIdleError::TimeoutError)
            );
            assert_eq!(request_info.unwrap().id, VALID_REQUEST_ID);
        },
    )
    .await;
}
