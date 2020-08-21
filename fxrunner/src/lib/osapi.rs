// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Traits for interacting safely with OS-level APIs.

use std::error::Error;
use std::fmt::Debug;
use std::io;
use std::time::Duration;

use thiserror::Error;
use tokio::time::delay_for;

pub mod error;
mod handle;
mod perf;
mod shutdown;

pub use perf::IoCounters;

/// A trait providing the ability to restart the current machine.
pub trait ShutdownProvider: Debug {
    /// The error
    type Error: Error + 'static;

    /// Initiate a restart with the given reason.
    fn initiate_restart(&self, reason: &str) -> Result<(), Self::Error>;
}

/// A trait providing the ability to retrieve disk and CPU performance
/// information.
pub trait PerfProvider: Debug {
    /// The error type returned by [`get_disk_io_counters()`](trait.PerfProvider.html#method.get_disk_io_counters).
    type DiskIoError: Error + 'static;

    /// The error type returned by [`get_cpu_idle_time()`](trait.PerfProvider.html#method.get_cpu_idle_time).
    type CpuTimeError: Error + 'static;

    /// The number of attempts that [`cpu_and_disk_idle()`](fn.cpu_and_disk_idle.html) will make before timing out.
    const ATTEMPT_COUNT: usize = 30;

    /// Return raw read and write IO counters.
    fn get_disk_io_counters(&self) -> Result<IoCounters, Self::DiskIoError>;

    /// Return the percentage of the time that the CPU is idle.
    ///
    /// The returned value is between 0 and 1.
    fn get_cpu_idle_time(&self) -> Result<f64, Self::CpuTimeError>;
}

/// A [`ShutdownProvider`](trait.ShutdownProvider.html) that uses the Windows API.
#[derive(Debug, Default)]
pub struct WindowsShutdownProvider {
    /// Whether or not to skip the actual restart.
    #[cfg(debug_assertions)]
    skip_restart: bool,
}

#[cfg(debug_assertions)]
impl WindowsShutdownProvider {
    pub fn skipping_restart(skip_restart: bool) -> Self {
        let mut provider = WindowsShutdownProvider::default();
        provider.skip_restart = skip_restart;
        provider
    }
}

impl ShutdownProvider for WindowsShutdownProvider {
    type Error = shutdown::ShutdownError;

    #[cfg(debug_assertions)]
    fn initiate_restart(&self, reason: &str) -> Result<(), Self::Error> {
        if self.skip_restart {
            Ok(())
        } else {
            shutdown::initiate_restart(reason)
        }
    }

    #[cfg(not(debug_assertions))]
    fn initiate_restart(&self, reason: &str) -> Result<(), Self::Error> {
        shutdown::initiate_restart(reason)
    }
}

#[derive(Debug, Default)]
pub struct WindowsPerfProvider;

impl PerfProvider for WindowsPerfProvider {
    type DiskIoError = perf::DiskIoError;
    type CpuTimeError = io::Error;

    fn get_disk_io_counters(&self) -> Result<IoCounters, Self::DiskIoError> {
        perf::get_disk_io_counters()
    }

    fn get_cpu_idle_time(&self) -> Result<f64, Self::CpuTimeError> {
        perf::get_cpu_idle_time()
    }
}

#[derive(Debug, Error)]
pub enum WaitForIdleError<P>
where
    P: PerfProvider,
{
    #[error("timed out waiting for CPU and disk to become idle")]
    TimeoutError,

    #[error(transparent)]
    DiskIoError(P::DiskIoError),

    #[error(transparent)]
    CpuTimeError(P::CpuTimeError),
}

/// Wait for the CPU and disk to become idle.
pub async fn cpu_and_disk_idle<P>(p: &P) -> Result<(), WaitForIdleError<P>>
where
    P: PerfProvider,
{
    const TARGET_CPU_IDLE_PERCENTAGE: f64 = 0.95;

    let mut counters = p
        .get_disk_io_counters()
        .map_err(WaitForIdleError::DiskIoError)?;

    for _ in 0..P::ATTEMPT_COUNT {
        delay_for(Duration::from_millis(500)).await;

        let new_counters = p
            .get_disk_io_counters()
            .map_err(WaitForIdleError::DiskIoError)?;
        let idle = p
            .get_cpu_idle_time()
            .map_err(WaitForIdleError::CpuTimeError)?;

        let delta_reads = new_counters.reads - counters.reads;
        let delta_writes = new_counters.writes - counters.writes;

        if idle >= TARGET_CPU_IDLE_PERCENTAGE && delta_reads == 0 && delta_writes == 0 {
            return Ok(());
        }

        counters = new_counters;
    }

    Err(WaitForIdleError::TimeoutError)
}
