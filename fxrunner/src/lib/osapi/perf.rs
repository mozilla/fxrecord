// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::error::Error;
use std::ffi::CString;
use std::ptr::null_mut;
use std::u32;

use derive_more::Display;
use winapi::shared::minwindef::FILETIME;
use winapi::um::fileapi::{CreateFileA, OPEN_EXISTING};
use winapi::um::ioapiset::DeviceIoControl;
use winapi::um::processthreadsapi::GetSystemTimes;
use winapi::um::winioctl::{DISK_PERFORMANCE, IOCTL_DISK_PERFORMANCE};
use winapi::um::winnt::{FILE_SHARE_READ, FILE_SHARE_WRITE};

use crate::osapi::error::{get_last_error, WindowsError};
use crate::osapi::handle::Handle;

pub struct IoCounters {
    pub reads: u32,
    pub writes: u32,
}

#[derive(Debug, Display)]
enum DiskIoErrorKind {
    #[display(fmt = "could not open C:\\ drive")]
    NoLogicalCDrive,

    #[display(fmt = "could not retrieve IO counters for C:\\ drive")]
    IoCounterError,
}

#[derive(Debug, Display)]
#[display(fmt = "{}: {}", kind, source)]
pub struct DiskIoError {
    kind: DiskIoErrorKind,
    source: WindowsError,
}

impl Error for DiskIoError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.source)
    }
}

pub(super) fn get_disk_io_counters() -> Result<IoCounters, DiskIoError> {
    let mut disk_perf: DISK_PERFORMANCE = unsafe { std::mem::zeroed() };

    // Implementation detail: reference laptops have a SINGLE logical drive, C:\.
    let device_path = CString::new(r#"\\.\C:"#).unwrap();

    let handle = Handle::from(unsafe {
        CreateFileA(
            device_path.as_ptr(),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            null_mut(),
            OPEN_EXISTING,
            0,
            null_mut(),
        )
    });

    if handle.as_ptr().is_null() {
        // There is no C drive?
        return Err(DiskIoError {
            kind: DiskIoErrorKind::NoLogicalCDrive,
            source: get_last_error(),
        });
    }

    let rv = unsafe {
        let mut bytes: u32 = 0;
        DeviceIoControl(
            handle.as_ptr(),
            IOCTL_DISK_PERFORMANCE,
            null_mut(),
            0,
            &mut disk_perf as *mut DISK_PERFORMANCE as *mut _,
            std::mem::size_of::<DISK_PERFORMANCE>() as u32,
            &mut bytes as *mut _,
            null_mut(),
        )
    };

    if rv == 0 {
        return Err(DiskIoError {
            kind: DiskIoErrorKind::IoCounterError,
            source: get_last_error(),
        });
    }

    Ok(IoCounters {
        reads: disk_perf.ReadCount,
        writes: disk_perf.WriteCount,
    })
}

pub(super) fn get_cpu_idle_time() -> Result<f64, WindowsError> {
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

    let rv = unsafe {
        GetSystemTimes(
            &mut idle_time as *mut _,
            &mut kernel_time as *mut _,
            &mut user_time as *mut _,
        )
    };

    if rv == 0 {
        return Err(get_last_error());
    }

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
