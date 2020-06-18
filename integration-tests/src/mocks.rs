// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::cell::RefCell;

use libfxrecord::error::ErrorMessage;
use libfxrunner::osapi::{IoCounters, PerfProvider, ShutdownProvider};

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
pub struct TestPerfProvider {
    failure_mode: Option<PerfFailureMode>,
    io_counters: RefCell<IoCounters>,
}

#[derive(Debug)]
pub enum PerfFailureMode {
    DiskIoError(&'static str),
    CpuTimeError(&'static str),
    DiskNeverIdle,
    CpuNeverIdle,
}

impl TestPerfProvider {
    pub fn with_failure(mode: PerfFailureMode) -> Self {
        TestPerfProvider {
            io_counters: Default::default(),
            failure_mode: Some(mode),
        }
    }
}

impl PerfProvider for TestPerfProvider {
    type DiskIoError = ErrorMessage<&'static str>;
    type CpuTimeError = ErrorMessage<&'static str>;

    const ATTEMPT_COUNT: usize = 1;

    fn get_disk_io_counters(&self) -> Result<IoCounters, Self::DiskIoError> {
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
        match self.failure_mode {
            Some(PerfFailureMode::CpuTimeError(s)) => Err(ErrorMessage(s)),
            Some(PerfFailureMode::CpuNeverIdle) => Ok(0f64),
            _ => Ok(0.99f64),
        }
    }
}
