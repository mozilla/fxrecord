// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use libfxrecord::error::ErrorMessage;
use libfxrunner::osapi::{IoCounters, PerfProvider, ShutdownProvider};
use libfxrunner::taskcluster::Taskcluster;
use tokio::fs;

use crate::util::{test_dir, AssertInvoked};

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
