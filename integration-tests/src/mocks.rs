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
use libfxrunner::osapi::{IoCounters, PerfProvider, ShutdownProvider};
use libfxrunner::request::{
    NewRequestError, RequestInfo, RequestManager, ResumeRequestError, ResumeRequestErrorKind,
};
use libfxrunner::taskcluster::Taskcluster;
use tempfile::TempDir;
use tokio::fs;

use crate::util::{test_dir, touch, AssertInvoked};

/// The only valid request ID for TestRequestManager.
pub const VALID_REQUEST_ID: &str = "REQUESTID";

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
            None => test_dir().join("firefox.zip"),
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
pub enum RequestFailureMode {
    NewRequest(NewRequestError),
    ResumeRequest(ResumeRequestErrorKind),
    EnsureProfileDir(&'static str),
}

/// A handle to the internals of a TestRequestManager.
///
/// The `TempDir` for the [`TestRequestManager`][TestRequestManager] exists
/// inside the handle becuase we need to extend its lifetime to the end of the
/// runner closure passed to `run_proto_test`. By the time that closure is
/// executed, `RunnerProto::handle_request` has already completed and
/// therefore consumed the `TestRequestManager`. In order to inspect the result
/// of the request, we need to keep the directory around.
///
/// [TestRequestManager]: struct.TestRequestManager.html.
pub struct TestRequestManagerHandle {
    tempdir: TempDir,
    last_request_info: Mutex<Option<RequestInfo<'static>>>,
}

pub struct TestRequestManager {
    failure_mode: Option<RequestFailureMode>,

    // Internal details of the request manager that need to be kept alive after
    // the `TestRequestMangaer` is consumed.
    handle: Arc<TestRequestManagerHandle>,
}

impl TestRequestManagerHandle {
    pub fn last_request_info(&self) -> Option<RequestInfo<'static>> {
        self.last_request_info.lock().unwrap().take()
    }
}

impl Default for TestRequestManager {
    fn default() -> Self {
        let tempdir = TempDir::new().expect("could not create tempdir for TestRequestManager");
        Self {
            failure_mode: None,
            handle: Arc::new(TestRequestManagerHandle {
                tempdir,
                last_request_info: Mutex::new(None),
            }),
        }
    }
}

impl TestRequestManager {
    pub fn with_failure(failure_mode: RequestFailureMode) -> Self {
        let mut manager = Self::default();
        manager.failure_mode = Some(failure_mode);
        manager
    }

    pub fn handle(&self) -> Arc<TestRequestManagerHandle> {
        self.handle.clone()
    }
}

#[async_trait]
impl RequestManager for TestRequestManager {
    async fn new_request(&self) -> Result<RequestInfo<'static>, NewRequestError> {
        match self.failure_mode {
            Some(RequestFailureMode::NewRequest(ref err)) => Err(clone_new_request_err(err)),
            _ => {
                let request_info = RequestInfo {
                    id: Cow::Borrowed(VALID_REQUEST_ID),
                    path: self.handle.tempdir.path().join("request"),
                };

                fs::create_dir(&request_info.path).await.unwrap();

                *self.handle.last_request_info.lock().unwrap() = Some(request_info.clone());
                Ok(request_info)
            }
        }
    }

    async fn resume_request<'a>(
        &self,
        request_id: &'a str,
    ) -> Result<RequestInfo<'a>, ResumeRequestError> {
        if let Some(RequestFailureMode::ResumeRequest(ref kind)) = self.failure_mode {
            return Err(ResumeRequestError {
                request_id: request_id.into(),
                kind: kind.clone(),
            });
        } else if request_id != VALID_REQUEST_ID {
            return Err(ResumeRequestError {
                request_id: request_id.into(),
                kind: ResumeRequestErrorKind::InvalidId,
            });
        }

        let request_info = RequestInfo {
            id: Cow::Borrowed(VALID_REQUEST_ID),
            path: self.handle.tempdir.path().join("request"),
        };

        fs::create_dir(&request_info.path).await.unwrap();
        fs::create_dir(&request_info.path.join("profile"))
            .await
            .unwrap();
        fs::create_dir(&request_info.path.join("firefox"))
            .await
            .unwrap();
        touch(&request_info.path.join("firefox").join("firefox.exe"))
            .await
            .unwrap();

        *self.handle.last_request_info.lock().unwrap() = Some(request_info.clone());
        Ok(request_info)
    }

    async fn ensure_valid_profile_dir<'a>(
        &self,
        request_info: &RequestInfo<'a>,
    ) -> Result<PathBuf, io::Error> {
        assert_eq!(request_info.id, VALID_REQUEST_ID);

        if let Some(RequestFailureMode::EnsureProfileDir(msg)) = self.failure_mode {
            return Err(io::Error::new(io::ErrorKind::Other, msg));
        }

        {
            // We scope this to intentionally drop the lock guard before the
            // await because the inner type is not Send.
            let info = self.handle.last_request_info.lock().unwrap();
            assert!(info.is_some());
            assert_eq!(info.as_ref().unwrap().id, request_info.id);
            assert_eq!(info.as_ref().unwrap().path, request_info.path);
        };

        let profile_path = request_info.path.join("profile");

        fs::create_dir(&profile_path).await.unwrap();
        Ok(profile_path)
    }
}

fn clone_new_request_err(err: &NewRequestError) -> NewRequestError {
    match err {
        NewRequestError::TooManyAttempts(a) => NewRequestError::TooManyAttempts(*a),
        NewRequestError::Io(inner) => {
            // io::Error does not impl Clone, so we do a *good enough* clone. In
            // practice, since we are only going to be testing this with custom
            // IO errors, this is effectively a clone implementation.
            NewRequestError::Io(io::Error::new(inner.kind(), err.to_string()))
        }
    }
}
