// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::ffi::CString;
use std::io;
use std::ptr::null_mut;

use thiserror::Error;
use winapi::shared::minwindef::{BOOL, DWORD};
use winapi::shared::ntdef::{LPSTR, LUID};
use winapi::um::winnt::TOKEN_PRIVILEGES;
use winapi::um::{processthreadsapi, reason, securitybaseapi, winbase, winnt, winreg};

use crate::osapi::handle::Handle;

#[derive(Debug, Error)]
enum ShutdownErrorKind {
    #[error("could not open process token")]
    OpenProcessToken,
    #[error("Could not lookup shutdown privilege")]
    LookupPrivilegeValue,
    #[error("Could not aquire shutdown privilege")]
    AdjustTokenPrivileges,
    #[error("InitiateSystemShutdownExA failed")]
    InitiateSystemShutdown,
}

#[derive(Debug, Error)]
#[error("{}: {}", .kind, .source)]
pub struct ShutdownError {
    kind: ShutdownErrorKind,
    source: io::Error,
}

// See: https://docs.microsoft.com/en-us/windows/win32/shutdown/how-to-shut-down-the-system
pub(super) fn initiate_restart(reason: &str) -> Result<(), ShutdownError> {
    let mut token = Handle::null();
    let mut privs = unsafe { std::mem::zeroed::<TOKEN_PRIVILEGES>() };

    let name = CString::new(winnt::SE_SHUTDOWN_NAME).unwrap();
    let success = unsafe {
        processthreadsapi::OpenProcessToken(
            processthreadsapi::GetCurrentProcess(),
            winnt::TOKEN_ADJUST_PRIVILEGES | winnt::TOKEN_QUERY,
            token.as_out_ptr(),
        ) != 0
    };
    if !success {
        return Err(ShutdownError {
            kind: ShutdownErrorKind::OpenProcessToken,
            source: io::Error::last_os_error(),
        });
    }

    let success = unsafe {
        winbase::LookupPrivilegeValueA(
            null_mut(),
            name.as_ptr(),
            &mut privs.Privileges[0].Luid as *mut LUID,
        ) != 0
    };
    if !success {
        return Err(ShutdownError {
            kind: ShutdownErrorKind::LookupPrivilegeValue,
            source: io::Error::last_os_error(),
        });
    }

    privs.PrivilegeCount = 1;
    privs.Privileges[0].Attributes = winnt::SE_PRIVILEGE_ENABLED;

    let success = unsafe {
        securitybaseapi::AdjustTokenPrivileges(
            token.as_ptr(),
            false as BOOL,
            &mut privs as *mut TOKEN_PRIVILEGES,
            0 as DWORD,
            null_mut(),
            null_mut(),
        ) != 0
    };

    if !success {
        return Err(ShutdownError {
            kind: ShutdownErrorKind::AdjustTokenPrivileges,
            source: io::Error::last_os_error(),
        });
    }

    let reason = CString::new(reason).unwrap();
    let success = unsafe {
        winreg::InitiateSystemShutdownExA(
            // Shutdown this machine.
            null_mut(),
            // This casts a `*const c_char` to a `*mut c_char` but the API does
            // not modify the string.
            reason.as_ptr() as LPSTR,
            // A three second timeout gives us plenty of time to shutdown TCP
            // connections and exit cleanly.
            3,
            // Force apps to close.
            true as BOOL,
            // Reboot after shutdown.
            true as BOOL,
            reason::SHTDN_REASON_MINOR_OTHER | reason::SHTDN_REASON_FLAG_PLANNED,
        ) != 0
    };

    if !success {
        return Err(ShutdownError {
            kind: ShutdownErrorKind::InitiateSystemShutdown,
            source: io::Error::last_os_error(),
        });
    }

    Ok(())
}
