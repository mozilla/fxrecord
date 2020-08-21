// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::convert::TryFrom;
use std::ffi::CString;
use std::io;
use std::ptr::null_mut;
use std::u32;

use thiserror::Error;
use winapi::shared::minwindef::FILETIME;
use winapi::um::winioctl::DISK_PERFORMANCE;
use winapi::um::{fileapi, ioapiset, processthreadsapi, winioctl, winnt};

use crate::osapi::error::check_nonzero;
use crate::osapi::handle::Handle;

#[derive(Clone, Copy, Debug, Default)]
pub struct IoCounters {
    pub reads: u32,
    pub writes: u32,
}

#[derive(Debug, Error)]
enum DiskIoErrorKind {
    #[error("could not open C:\\ drive")]
    NoLogicalCDrive,

    #[error("could not retrieve IO counters for C:\\ drive")]
    IoCounterError,
}

#[derive(Debug, Error)]
#[error("{}: {}", .kind, .source)]
pub struct DiskIoError {
    kind: DiskIoErrorKind,
    source: io::Error,
}

pub(super) fn get_disk_io_counters() -> Result<IoCounters, DiskIoError> {
    let mut disk_perf: DISK_PERFORMANCE = unsafe { std::mem::zeroed() };

    // Implementation detail: reference laptops have a SINGLE logical drive, C:\.
    let device_path = CString::new(r#"\\.\C:"#).unwrap();

    let handle = Handle::try_from(unsafe {
        fileapi::CreateFileA(
            device_path.as_ptr(),
            0,
            winnt::FILE_SHARE_READ | winnt::FILE_SHARE_WRITE,
            null_mut(),
            fileapi::OPEN_EXISTING,
            0,
            null_mut(),
        )
    })
    .map_err(|source| DiskIoError {
        kind: DiskIoErrorKind::NoLogicalCDrive,
        source,
    })?;

    let mut bytes: u32 = 0;
    check_nonzero(unsafe {
        ioapiset::DeviceIoControl(
            handle.as_ptr(),
            winioctl::IOCTL_DISK_PERFORMANCE,
            null_mut(),
            0,
            &mut disk_perf as *mut DISK_PERFORMANCE as *mut _,
            std::mem::size_of::<DISK_PERFORMANCE>() as u32,
            &mut bytes as *mut _,
            null_mut(),
        )
    })
    .map_err(|source| DiskIoError {
        kind: DiskIoErrorKind::IoCounterError,
        source,
    })?;

    Ok(IoCounters {
        reads: disk_perf.ReadCount,
        writes: disk_perf.WriteCount,
    })
}

pub(super) fn get_cpu_idle_time() -> Result<f64, io::Error> {
    let mut idle_time = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let mut kernel_time = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let mut user_time = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };

    check_nonzero(unsafe {
        processthreadsapi::GetSystemTimes(
            &mut idle_time as *mut _,
            &mut kernel_time as *mut _,
            &mut user_time as *mut _,
        )
    })?;

    let idle_time = get_filetime_as_u64(idle_time) as f64;
    let kernel_time = get_filetime_as_u64(kernel_time) as f64;
    let user_time = get_filetime_as_u64(user_time) as f64;

    // Kernel time includes idle time.
    // See documentation of `lpKerneltime` here:
    // https://docs.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getsystemtimes
    let total_time = kernel_time + user_time;

    Ok(idle_time / total_time)
}

// Return the given `FILETIME` as a u64 of 10^{-7} seconds.
fn get_filetime_as_u64(t: FILETIME) -> u64 {
    // The FILETIME structure is represented as a high word (u32) and low word.
    // The low word is expressed in units of 10^{-7} seconds and the high word
    // is expressed in units of std::u32::MAX * 10^{-7} seconds.

    t.dwHighDateTime as u64 * u32::MAX as u64 + t.dwLowDateTime as u64
}
