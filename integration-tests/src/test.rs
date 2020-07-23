// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

mod mocks;
mod util;

use std::convert::TryInto;
use std::fs::File;
use std::future::Future;

use assert_matches::assert_matches;
use futures::join;
use indoc::indoc;
use libfxrecord::net::*;
use libfxrecorder::proto::{RecorderProto, RecorderProtoError};
use libfxrunner::osapi::WaitForIdleError;
use libfxrunner::proto::{RunnerProto, RunnerProtoError};
use libfxrunner::session::{
    NewSessionError, ResumeSessionError, ResumeSessionErrorKind, SessionInfo,
};
use libfxrunner::zip::ZipError;
use serde_json::{json, Value};
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
    RunnerProto<TestShutdownProvider, TestTaskcluster, TestPerfProvider, TestSessionManager>;
type TestRunnerProtoError =
    RunnerProtoError<TestShutdownProvider, TestTaskcluster, TestPerfProvider>;

struct RunnerInfo {
    result: Result<bool, TestRunnerProtoError>,
    session_info: Option<SessionInfo<'static>>,
}

/// Run a test with both the recorder and runner protocols.
async fn run_proto_test<'a, Fut>(
    listener: &mut TcpListener,
    shutdown_provider: TestShutdownProvider,
    tc: TestTaskcluster,
    perf_provider: TestPerfProvider,
    session_manager: TestSessionManager,
    recorder_fn: impl FnOnce(RecorderProto) -> Fut,
    runner_fn: impl FnOnce(RunnerInfo),
) where
    Fut: Future<Output = ()>,
{
    let addr = listener.local_addr().unwrap();

    let runner = async {
        let (stream, _) = listener.accept().await.unwrap();

        let handle = session_manager.handle();

        let result = TestRunnerProto::handle_request(
            test_logger(),
            stream,
            shutdown_provider,
            tc,
            perf_provider,
            session_manager,
        )
        .await;

        runner_fn(RunnerInfo {
            result,
            session_info: handle.last_session_info(),
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
async fn test_new_session_ok() {
    let mut listener = TcpListener::bind("127.0.0.1:0").await.unwrap();

    run_proto_test(
        &mut listener,
        TestShutdownProvider::default(),
        TestTaskcluster::default(),
        TestPerfProvider::default(),
        TestSessionManager::default(),
        |mut recorder| async move {
            assert_eq!(
                recorder.new_session("task_id", None, vec![]).await.unwrap(),
                VALID_SESSION_ID
            );
        },
        |RunnerInfo {
             result,
             session_info,
         }| {
            assert_eq!(result.unwrap(), true);

            let session_info = session_info.unwrap();
            let firefox_dir = session_info.path.join("firefox");
            assert!(firefox_dir
                .join("firefox.exe")
                .is_file());

            let dist_path = firefox_dir.join("distribution");
            assert!(dist_path.is_dir());
            let policies: Value = {
                let f = File::open(dist_path.join("policies.json")).unwrap();
                serde_json::from_reader(f).unwrap()
            };

            assert_eq!(policies, json!({
                "policies": {
                    "DisableAppUpdate": true
                }
            }));

            let profile_dir = session_info.path.join("profile");
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
        TestSessionManager::default(),
        |mut recorder| async move {
            assert_eq!(
                recorder
                    .new_session("task_id", Some(&test_dir().join("profile.zip")), vec![])
                    .await
                    .unwrap(),
                VALID_SESSION_ID
            );
        },
        |RunnerInfo {
             result,
             session_info,
         }| {
            assert_eq!(result.unwrap(), true);

            let session_info = session_info.unwrap();
            assert!(session_info
                .path
                .join("firefox")
                .join("firefox.exe")
                .is_file());

            let profile_dir = session_info.path.join("profile");
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
        TestSessionManager::default(),
        |mut recorder| async move {
            let session_id = recorder
                .new_session(
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

            assert_eq!(session_id, VALID_SESSION_ID);
        },
        |RunnerInfo {
             result,
             session_info,
         }| {
            assert_eq!(result.unwrap(), true);

            let session_info = session_info.unwrap();
            assert!(session_info
                .path
                .join("firefox")
                .join("firefox.exe")
                .is_file());

            let profile_dir = session_info.path.join("profile");
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
        TestSessionManager::default(),
        |mut recorder| async move {
            let session_id = recorder
                .new_session(
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

            assert_eq!(session_id, VALID_SESSION_ID);
        },
        |RunnerInfo {
             result,
             session_info,
         }| {
            assert_eq!(result.unwrap(), true);

            let session_info = session_info.unwrap();
            assert!(session_info
                .path
                .join("firefox")
                .join("firefox.exe")
                .is_file());

            let profile_dir = session_info.path.join("profile");
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
async fn test_new_session_err_request_manager() {
    let mut listener = TcpListener::bind("127.0.0.1:0").await.unwrap();

    run_proto_test(
        &mut listener,
        TestShutdownProvider::default(),
        TestTaskcluster::default(),
        TestPerfProvider::default(),
        TestSessionManager::with_failure(SessionFailureMode::NewSession(
            NewSessionError::TooManyAttempts(32),
        )),
        |mut recorder| async move {
            assert_matches!(
                recorder.new_session("task_id", None, vec![]).await.unwrap_err(),
                RecorderProtoError::Proto(ProtoError::Foreign(e)) => {
                    assert_eq!(
                        e.to_string(),
                        "Could not create a request directory after 32 attempts");
                }
            );
        },
        |RunnerInfo {
             result,
             session_info,
         }| {
            assert_matches!(
                result.unwrap_err(),
                RunnerProtoError::NewSession(NewSessionError::TooManyAttempts(32))
            );

            assert!(session_info.is_none());
        },
    )
    .await;

    run_proto_test(
        &mut listener,
        TestShutdownProvider::default(),
        TestTaskcluster::default(),
        TestPerfProvider::default(),
        TestSessionManager::with_failure(SessionFailureMode::EnsureProfileDir(
            "could not ensure profile directory",
        )),
        |mut recorder| async move {
            assert_matches!(
                recorder.new_session("task_id", None, vec![]).await.unwrap_err(),
                RecorderProtoError::Proto(ProtoError::Foreign(e)) => {
                    assert_eq!(
                        e.to_string(),
                        "could not ensure profile directory");
                }
            );
        },
        |RunnerInfo {
             result,
             session_info,
         }| {
            assert_matches!(
                result.unwrap_err(),
                RunnerProtoError::EnsureProfile(e) => {
                    assert_eq!(e.to_string(), "could not ensure profile directory");
                }
            );

            let session_info = session_info.unwrap();
            assert_eq!(session_info.id, VALID_SESSION_ID);
            assert!(!session_info.path.exists());
        },
    )
    .await;
}

#[tokio::test]
async fn test_new_session_err_downloadbuild() {
    let mut listener = TcpListener::bind("127.0.0.1:0").await.unwrap();

    run_proto_test(
        &mut listener,
        TestShutdownProvider::default(),
        TestTaskcluster::with_failure(TaskclusterFailureMode::BadZip),
        TestPerfProvider::default(),
        TestSessionManager::default(),
        |mut recorder| async move {
            assert_matches!(
                recorder
                    .new_session("task_id", None, vec![])
                    .await
                    .unwrap_err(),
                RecorderProtoError::Proto(ProtoError::Foreign(e)) => {
                    assert_eq!(e.to_string(), TestRunnerProtoError::MissingFirefox.to_string());
                }
            );
        },
        |RunnerInfo {
             result,
             session_info,
         }| {
            assert_matches!(result.unwrap_err(), RunnerProtoError::MissingFirefox);

            let session_info = session_info.unwrap();
            assert_eq!(session_info.id, VALID_SESSION_ID);

            assert!(!session_info.path.exists());
        },
    )
    .await;

    run_proto_test(
        &mut listener,
        TestShutdownProvider::default(),
        TestTaskcluster::with_failure(TaskclusterFailureMode::Generic("404 Not Found")),
        TestPerfProvider::default(),
        TestSessionManager::default(),
        |mut recorder| async move {
            assert_matches!(
                recorder
                    .new_session("task_id", None, vec![])
                    .await
                    .unwrap_err(),
                RecorderProtoError::Proto(ProtoError::Foreign(e)) => {
                     assert_eq!(e.to_string(), "404 Not Found");
                }
            );
        },
        |RunnerInfo {
             result,
             session_info,
         }| {
            assert_matches!(
                result.unwrap_err(),
                RunnerProtoError::Taskcluster(e) => {
                    assert_eq!(e.to_string(), "404 Not Found");
                }
            );

            let session_info = session_info.unwrap();
            assert_eq!(session_info.id, VALID_SESSION_ID);
            assert!(!session_info.path.exists());
        },
    )
    .await;

    run_proto_test(
        &mut listener,
        TestShutdownProvider::default(),
        TestTaskcluster::with_failure(TaskclusterFailureMode::NotZip),
        TestPerfProvider::default(),

        TestSessionManager::default(),
        |mut recorder| async move {
            assert_matches!(
                recorder
                    .new_session("task_id", None, vec![])
                    .await
                    .unwrap_err(),
                RecorderProtoError::Proto(ProtoError::Foreign(e)) => {
                    let msg = e.to_string();
                    assert!(msg.starts_with("could not read zip archive"));
                    assert!(msg.ends_with("Invalid Zip archive: Could not find central directory end"));
                }
            );
        },
        |RunnerInfo { result, session_info }| {
            let session_info = session_info.unwrap();
            assert_eq!(session_info.id, VALID_SESSION_ID);

            assert_matches!(
                result.unwrap_err(),
                RunnerProtoError::Zip(e @ ZipError::ReadArchive{ .. }) => {

                    assert_eq!(
                        e.to_string(),
                        format!(
                            "could not read zip archive `{}': Invalid Zip archive: Could not find central directory end",
                            session_info.path.join("firefox.zip").display()
                        )
                    );
                }
            );

            assert!(!session_info.path.exists());
        },
    )
    .await;
}

#[tokio::test]
async fn test_new_session_err_recvprofile() {
    let mut listener = TcpListener::bind("127.0.0.1:0").await.unwrap();

    run_proto_test(
        &mut listener,
        TestShutdownProvider::default(),
        TestTaskcluster::default(),
        TestPerfProvider::default(),

        TestSessionManager::default(),
        |mut recorder| async move {
            assert_matches!(
                recorder
                    .new_session("task_id", Some(&test_dir().join("README.md")), vec![])
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
        |RunnerInfo { result, session_info }| {
            let session_info = session_info.unwrap();
            assert_eq!(session_info.id, VALID_SESSION_ID);

            assert_matches!(
                result.unwrap_err(),
                RunnerProtoError::Zip(e @ ZipError::ReadArchive{ .. }) => {
                    assert_eq!(
                        e.to_string(),
                        format!(
                            "could not read zip archive `{}': Invalid Zip archive: Could not find central directory end",
                            session_info.path.join("profile.zip").display()
                        )
                    );
                }
            );

            assert!(!session_info.path.exists());
        },
    )
    .await;

    run_proto_test(
        &mut listener,
        TestShutdownProvider::default(),
        TestTaskcluster::default(),
        TestPerfProvider::default(),
        TestSessionManager::default(),
        |mut recorder| async move {
            assert_matches!(
                recorder
                    .new_session("task_id", Some(&test_dir().join("empty.zip")), vec![])
                    .await
                    .unwrap_err(),
                RecorderProtoError::Proto(ProtoError::Foreign(e)) => {
                    assert_eq!(e.to_string(), "An empty profile was received");
                }
            );
        },
        |RunnerInfo {
             result,
             session_info,
         }| {
            assert_matches!(result.unwrap_err(), RunnerProtoError::EmptyProfile);

            let session_info = session_info.unwrap();
            assert!(!session_info.path.exists());
        },
    )
    .await;
}

#[tokio::test]
async fn test_new_session_err_restarting() {
    let mut listener = TcpListener::bind("127.0.0.1:0").await.unwrap();

    run_proto_test(
        &mut listener,
        TestShutdownProvider::with_error("could not shut down"),
        TestTaskcluster::default(),
        TestPerfProvider::default(),
        TestSessionManager::default(),
        |mut recorder| async move {
            assert_matches!(
                recorder.new_session("task_id", None, vec![])
                    .await
                    .unwrap_err(),
                RecorderProtoError::Proto(ProtoError::Foreign(e)) => {
                    assert_eq!(e.to_string(), "could not shut down");
                }
            );
        },
        |RunnerInfo {
             result,
             session_info,
         }| {
            assert_matches!(
                result.unwrap_err(),
                RunnerProtoError::Shutdown(e) => {
                    assert_eq!(e.to_string(), "could not shut down")
                }
            );

            let session_info = session_info.unwrap();
            assert!(!session_info.path.exists());
        },
    )
    .await;
}

#[tokio::test]
async fn test_resume_session_ok() {
    let mut listener = TcpListener::bind("127.0.0.1:0").await.unwrap();

    run_proto_test(
        &mut listener,
        TestShutdownProvider::default(),
        TestTaskcluster::default(),
        TestPerfProvider::asserting_invoked(),
        TestSessionManager::default(),
        |mut recorder| async move {
            recorder
                .resume_session(VALID_SESSION_ID, Idle::Wait)
                .await
                .unwrap();
        },
        |RunnerInfo {
             result,
             session_info,
         }| {
            assert_eq!(result.unwrap(), false);
            assert_eq!(session_info.unwrap().id, VALID_SESSION_ID);
        },
    )
    .await;

    run_proto_test(
        &mut listener,
        TestShutdownProvider::default(),
        TestTaskcluster::default(),
        TestPerfProvider::asserting_not_invoked(),
        TestSessionManager::default(),
        |mut recorder| async move {
            recorder
                .resume_session(VALID_SESSION_ID, Idle::Skip)
                .await
                .unwrap();
        },
        |RunnerInfo {
             result,
             session_info,
         }| {
            assert_eq!(result.unwrap(), false);
            assert_eq!(session_info.unwrap().id, VALID_SESSION_ID);
        },
    )
    .await;
}

#[tokio::test]
async fn test_resume_session_err_request_manager() {
    let mut listener = TcpListener::bind("127.0.0.1:0").await.unwrap();

    run_proto_test(
        &mut listener,
        TestShutdownProvider::default(),
        TestTaskcluster::default(),
        TestPerfProvider::default(),
        TestSessionManager::default(),
        |mut recorder| async move {
            assert_matches!(
                // Any request that is not VALID_REQUEST_ID triggers this error.
                recorder.resume_session("foobar", Idle::Skip).await.unwrap_err(),
                RecorderProtoError::Proto(ProtoError::Foreign(e)) => {
                    assert_eq!(e.to_string(), "Invalid session ID `foobar': ID contains invalid characters");
                }
            );
        },
        |RunnerInfo {
             result,
             session_info,
         }| {
             assert_matches!(
                result.unwrap_err(),
                RunnerProtoError::ResumeSession(e) => {
                    assert_eq!(e, ResumeSessionError {
                        kind: ResumeSessionErrorKind::InvalidId,
                        session_id: "foobar".into(),
                    });
                }
            );

            assert!(session_info.is_none());
         },
    )
    .await;

    run_proto_test(
        &mut listener,
        TestShutdownProvider::default(),
        TestTaskcluster::default(),
        TestPerfProvider::default(),
        TestSessionManager::with_failure(SessionFailureMode::ResumeSession(
            ResumeSessionErrorKind::MissingProfile,
        )),
        |mut recorder| async move {
            assert_matches!(
                recorder
                    .resume_session(VALID_SESSION_ID, Idle::Skip)
                    .await
                    .unwrap_err(),
                RecorderProtoError::Proto(ProtoError::Foreign(e)) => {
                    assert_eq!(
                        e.to_string(),
                        "Invalid session ID `REQUESTID': missing a profile directory"
                    );
                }
            );
        },
        |RunnerInfo {
             result,
             session_info,
         }| {
            assert_matches!(
                result.unwrap_err(),
                RunnerProtoError::ResumeSession(e) => {
                    assert_eq!(
                        e,
                        ResumeSessionError {
                            kind: ResumeSessionErrorKind::MissingProfile,
                            session_id: VALID_SESSION_ID.into(),
                        }
                    );
                }
            );

            assert!(session_info.is_none());
        },
    )
    .await;
}

#[tokio::test]
async fn test_resume_session_err_waitforidle() {
    let mut listener = TcpListener::bind("127.0.0.1:0").await.unwrap();

    run_proto_test(
        &mut listener,
        TestShutdownProvider::default(),
        TestTaskcluster::default(),
        TestPerfProvider::with_failure(PerfFailureMode::DiskIoError("disk io error")),
        TestSessionManager::default(),
        |mut recorder| async move {
            assert_matches!(
                recorder
                    .resume_session(VALID_SESSION_ID, Idle::Wait)
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
             session_info,
         }| {
            assert_matches!(
                result.unwrap_err(),
                RunnerProtoError::WaitForIdle(WaitForIdleError::DiskIoError(e)) => {
                    assert_eq!(e.to_string(), "disk io error");
                }
            );

            assert_eq!(session_info.unwrap().id, VALID_SESSION_ID);
        },
    )
    .await;

    run_proto_test(
        &mut listener,
        TestShutdownProvider::default(),
        TestTaskcluster::default(),
        TestPerfProvider::with_failure(PerfFailureMode::CpuTimeError("cpu time error")),
        TestSessionManager::default(),
        |mut recorder| async move {
            assert_matches!(
                recorder
                    .resume_session(VALID_SESSION_ID, Idle::Wait)
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
             session_info,
         }| {
            assert_matches!(
                result.unwrap_err(),
                RunnerProtoError::WaitForIdle(WaitForIdleError::CpuTimeError(e)) => {
                    assert_eq!(e.to_string(), "cpu time error");
                }
            );
            assert_eq!(session_info.unwrap().id, VALID_SESSION_ID);
        },
    )
    .await;

    run_proto_test(
        &mut listener,
        TestShutdownProvider::default(),
        TestTaskcluster::default(),
        TestPerfProvider::with_failure(PerfFailureMode::DiskNeverIdle),
        TestSessionManager::default(),
        |mut recorder| async move {
            assert_matches!(
                recorder
                    .resume_session(VALID_SESSION_ID, Idle::Wait)
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
             session_info,
         }| {
            assert_matches!(
                result.unwrap_err(),
                RunnerProtoError::WaitForIdle(WaitForIdleError::TimeoutError)
            );
            assert_eq!(session_info.unwrap().id, VALID_SESSION_ID);
        },
    )
    .await;

    run_proto_test(
        &mut listener,
        TestShutdownProvider::default(),
        TestTaskcluster::default(),
        TestPerfProvider::with_failure(PerfFailureMode::CpuNeverIdle),
        TestSessionManager::default(),
        |mut recorder| async move {
            assert_matches!(
                recorder
                    .resume_session(VALID_SESSION_ID, Idle::Wait)
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
             session_info,
         }| {
            assert_matches!(
                result.unwrap_err(),
                RunnerProtoError::WaitForIdle(WaitForIdleError::TimeoutError)
            );
            assert_eq!(session_info.unwrap().id, VALID_SESSION_ID);
        },
    )
    .await;
}
