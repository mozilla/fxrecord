// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::borrow::Cow;
use std::cell::RefCell;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use libfxrecord::error::ErrorMessage;
use libfxrecorder::recorder::Recorder;
use libfxrunner::osapi::{IoCounters, PerfProvider, ShutdownProvider};
use libfxrunner::session::{
    NewSessionError, ResumeSessionError, ResumeSessionErrorKind, SessionInfo, SessionManager,
};
use libfxrunner::splash::Splash;
use libfxrunner::taskcluster::Taskcluster;
use tempfile::TempDir;
use tokio::fs;

use crate::util::{firefox_zip_path, test_dir, AssertInvoked};

/// The only valid session ID for TestSessionManager.
pub const VALID_SESSION_ID: &str = "REQUESTID";

#[derive(Debug, Default)]
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

#[derive(Debug, Default)]
pub struct TestTaskcluster {
    failure_mode: Option<TaskclusterFailureMode>,
}

#[derive(Debug)]
pub enum TaskclusterFailureMode {
    BadZip,
    NotZip,
    Generic(&'static str),
}

impl TestTaskcluster {
    pub fn with_failure(failure_mode: TaskclusterFailureMode) -> Self {
        Self {
            failure_mode: Some(failure_mode),
        }
    }
}

#[async_trait]
impl Taskcluster for TestTaskcluster {
    type Error = ErrorMessage<&'static str>;

    async fn download_build_artifact(
        &mut self,
        _task_id: &str,
        download_dir: &Path,
    ) -> Result<PathBuf, Self::Error> {
        let zip_path = match self.failure_mode {
            Some(TaskclusterFailureMode::Generic(e)) => {
                return Err(ErrorMessage(e));
            }
            Some(TaskclusterFailureMode::BadZip) => test_dir().join("test.zip"),
            Some(TaskclusterFailureMode::NotZip) => test_dir().join("README.md"),
            None => firefox_zip_path(),
        };

        let dest = download_dir.join("firefox.zip");

        assert!(zip_path.exists());

        fs::copy(&zip_path, &dest).await.unwrap();

        Ok(dest)
    }
}

#[derive(Debug)]
pub enum PerfFailureMode {
    DiskIoError(&'static str),
    CpuTimeError(&'static str),
    DiskNeverIdle,
    CpuNeverIdle,
}

#[derive(Debug)]
pub struct TestPerfProvider {
    failure_mode: Option<PerfFailureMode>,
    io_counters: RefCell<IoCounters>,
    assert_invoked: Option<RefCell<AssertInvoked>>,
}

impl Default for TestPerfProvider {
    fn default() -> Self {
        TestPerfProvider {
            io_counters: Default::default(),
            failure_mode: None,
            assert_invoked: None,
        }
    }
}

impl TestPerfProvider {
    pub fn with_failure(mode: PerfFailureMode) -> Self {
        TestPerfProvider {
            io_counters: Default::default(),
            failure_mode: Some(mode),
            assert_invoked: Some(RefCell::new(AssertInvoked::new("TestPerfProvider", true))),
        }
    }

    pub fn asserting_invoked() -> Self {
        TestPerfProvider {
            io_counters: Default::default(),
            failure_mode: None,
            assert_invoked: Some(RefCell::new(AssertInvoked::new("TestPerfProvider", true))),
        }
    }

    pub fn asserting_not_invoked() -> Self {
        TestPerfProvider {
            io_counters: Default::default(),
            failure_mode: None,
            assert_invoked: Some(RefCell::new(AssertInvoked::new("TestPerfProvider", false))),
        }
    }

    fn invoked(&self) {
        if let Some(ai) = self.assert_invoked.as_ref() {
            ai.borrow_mut().invoked();
        }
    }
}

impl PerfProvider for TestPerfProvider {
    type DiskIoError = ErrorMessage<&'static str>;
    type CpuTimeError = ErrorMessage<&'static str>;

    const ATTEMPT_COUNT: usize = 1;

    fn get_disk_io_counters(&self) -> Result<IoCounters, Self::DiskIoError> {
        self.invoked();

        match self.failure_mode {
            Some(PerfFailureMode::DiskIoError(s)) => Err(ErrorMessage(s)),
            Some(PerfFailureMode::DiskNeverIdle) => {
                let mut io_counters = self.io_counters.borrow_mut();

                io_counters.reads += 1;
                io_counters.writes += 1;

                Ok(*io_counters)
            }
            _ => Ok(*self.io_counters.borrow()),
        }
    }

    fn get_cpu_idle_time(&self) -> Result<f64, Self::CpuTimeError> {
        self.invoked();

        match self.failure_mode {
            Some(PerfFailureMode::CpuTimeError(s)) => Err(ErrorMessage(s)),
            Some(PerfFailureMode::CpuNeverIdle) => Ok(0f64),
            _ => Ok(0.99f64),
        }
    }
}

#[derive(Debug)]
pub enum SessionFailureMode {
    NewSession(NewSessionError),
    ResumeSession(ResumeSessionErrorKind),
    EnsureProfileDir(&'static str),
}

/// A handle to the internals of a TestSessionManager.
///
/// The `TempDir` for the [`TestSessionManager`][TestSessionManager] exists
/// inside the handle becuase we need to extend its lifetime to the end of the
/// runner closure passed to `run_proto_test`. By the time that closure is
/// executed, `RunnerProto::handle_request` has already completed and
/// therefore consumed the `TestSessionManager`. In order to inspect the result
/// of the request, we need to keep the directory around.
///
/// [TestSessionManager]: struct.TestSessionManager.html.
pub struct TestSessionManagerHandle {
    tempdir: TempDir,
    last_session_info: Mutex<Option<SessionInfo<'static>>>,
}

pub struct TestSessionManager {
    failure_mode: Option<SessionFailureMode>,

    // Internal details of the session manager that need to be kept alive after
    // the `TestSessionMangaer` is consumed.
    handle: Arc<TestSessionManagerHandle>,
}

impl TestSessionManagerHandle {
    pub fn last_session_info(&self) -> Option<SessionInfo<'static>> {
        self.last_session_info.lock().unwrap().take()
    }
}

impl Default for TestSessionManager {
    fn default() -> Self {
        let tempdir = TempDir::new().expect("could not create tempdir for TestSessionManager");
        Self {
            failure_mode: None,
            handle: Arc::new(TestSessionManagerHandle {
                tempdir,
                last_session_info: Mutex::new(None),
            }),
        }
    }
}

impl TestSessionManager {
    pub fn with_failure(failure_mode: SessionFailureMode) -> Self {
        let mut manager = Self::default();
        manager.failure_mode = Some(failure_mode);
        manager
    }

    pub fn handle(&self) -> Arc<TestSessionManagerHandle> {
        self.handle.clone()
    }
}

#[async_trait]
impl SessionManager for TestSessionManager {
    async fn new_session(&self) -> Result<SessionInfo<'static>, NewSessionError> {
        match self.failure_mode {
            Some(SessionFailureMode::NewSession(ref err)) => Err(clone_new_session_err(err)),
            _ => {
                let session_info = SessionInfo {
                    id: Cow::Borrowed(VALID_SESSION_ID),
                    path: self.handle.tempdir.path().join("session"),
                };

                fs::create_dir(&session_info.path).await.unwrap();

                *self.handle.last_session_info.lock().unwrap() = Some(session_info.clone());
                Ok(session_info)
            }
        }
    }

    async fn resume_session<'a>(
        &self,
        session_id: &'a str,
    ) -> Result<SessionInfo<'a>, ResumeSessionError> {
        if let Some(SessionFailureMode::ResumeSession(ref kind)) = self.failure_mode {
            return Err(ResumeSessionError {
                session_id: session_id.into(),
                kind: kind.clone(),
            });
        } else if session_id != VALID_SESSION_ID {
            return Err(ResumeSessionError {
                session_id: session_id.into(),
                kind: ResumeSessionErrorKind::InvalidId,
            });
        }

        let session_info = SessionInfo {
            id: Cow::Borrowed(VALID_SESSION_ID),
            path: self.handle.tempdir.path().join("session"),
        };

        fs::create_dir(&session_info.path).await.unwrap();
        fs::create_dir(&session_info.path.join("profile"))
            .await
            .unwrap();

        libfxrunner::zip::unzip(&firefox_zip_path(), &session_info.path).unwrap();

        *self.handle.last_session_info.lock().unwrap() = Some(session_info.clone());
        Ok(session_info)
    }

    async fn ensure_valid_profile_dir<'a>(
        &self,
        session_info: &SessionInfo<'a>,
    ) -> Result<PathBuf, io::Error> {
        assert_eq!(session_info.id, VALID_SESSION_ID);

        if let Some(SessionFailureMode::EnsureProfileDir(msg)) = self.failure_mode {
            return Err(io::Error::new(io::ErrorKind::Other, msg));
        }

        {
            // We scope this to intentionally drop the lock guard before the
            // await because the inner type is not Send.
            let info = self.handle.last_session_info.lock().unwrap();
            assert!(info.is_some());
            assert_eq!(info.as_ref().unwrap().id, session_info.id);
            assert_eq!(info.as_ref().unwrap().path, session_info.path);
        };

        let profile_path = session_info.path.join("profile");

        fs::create_dir(&profile_path).await.unwrap();
        Ok(profile_path)
    }
}

fn clone_new_session_err(err: &NewSessionError) -> NewSessionError {
    match err {
        NewSessionError::TooManyAttempts(a) => NewSessionError::TooManyAttempts(*a),
        NewSessionError::Io(inner) => {
            // io::Error does not impl Clone, so we do a *good enough* clone. In
            // practice, since we are only going to be testing this with custom
            // IO errors, this is effectively a clone implementation.
            NewSessionError::Io(io::Error::new(inner.kind(), err.to_string()))
        }
    }
}

pub struct TestSplash;

#[async_trait]
impl Splash for TestSplash {
    async fn new(_display_width: u32, _display_height: u32) -> Result<Self, io::Error> {
        Ok(TestSplash)
    }

    fn destroy(&mut self) -> Result<(), io::Error> {
        Ok(())
    }
}

pub struct TestRecorder;
pub struct TestRecorderHandle(PathBuf);

#[async_trait]
impl Recorder for TestRecorder {
    type Error = io::Error;
    type Handle = TestRecorderHandle;

    async fn start_recording(&self, directory: &Path) -> Result<Self::Handle, Self::Error> {
        Ok(TestRecorderHandle(directory.join("recording.mp4")))
    }

    async fn wait_for_recording_finished(
        &self,
        handle: Self::Handle,
    ) -> Result<PathBuf, Self::Error> {
        Ok(handle.0)
    }
}
