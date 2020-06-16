// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::error::Error;
use std::ffi::CString;
use std::ptr::null_mut;

use derive_more::Display;
use winapi::shared::minwindef::{BOOL, DWORD};
use winapi::shared::ntdef::{LPSTR, LUID};
use winapi::um::processthreadsapi::{GetCurrentProcess, OpenProcessToken};
use winapi::um::reason::{SHTDN_REASON_FLAG_PLANNED, SHTDN_REASON_MINOR_OTHER};
use winapi::um::securitybaseapi::AdjustTokenPrivileges;
use winapi::um::winbase::LookupPrivilegeValueA;
use winapi::um::winnt::{
    SE_PRIVILEGE_ENABLED, SE_SHUTDOWN_NAME, TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
};
use winapi::um::winreg::InitiateSystemShutdownExA;

use crate::osapi::error::{get_last_error, WindowsError};
use crate::osapi::handle::Handle;

#[derive(Debug, Display)]
enum ShutdownErrorKind {
    #[display(fmt = "could not open process token")]
    OpenProcessToken,
    #[display(fmt = "Could not lookup shutdown privilege")]
    LookupPrivilegeValue,
    #[display(fmt = "Could not aquire shutdown privilege")]
    AdjustTokenPrivileges,
    #[display(fmt = "InitiateSystemShutdownExA failed")]
    InitiateSystemShutdown,
}

#[derive(Debug, Display)]
#[display(fmt = "{}: {}", kind, source)]
pub struct ShutdownError {
    kind: ShutdownErrorKind,
    source: WindowsError,
}

impl Error for ShutdownError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.source)
    }
}

// See: https://docs.microsoft.com/en-us/windows/win32/shutdown/how-to-shut-down-the-system
pub(super) fn initiate_restart(reason: &str) -> Result<(), ShutdownError> {
    let mut token = Handle::null();
    let mut privs = unsafe { std::mem::zeroed::<TOKEN_PRIVILEGES>() };

    let name = CString::new(SE_SHUTDOWN_NAME).unwrap();
    let success = unsafe {
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
            token.as_out_ptr(),
        ) != 0
    };
    if !success {
        return Err(ShutdownError {
            kind: ShutdownErrorKind::OpenProcessToken,
            source: get_last_error(),
        });
    }

    let success = unsafe {
        // We cast `name`'s underlying pointer to `LPSTR` (`*mut c_char`) here,
        // but it is safe because it will not be modified through the pointer.
        LookupPrivilegeValueA(
            null_mut(),
            name.as_ptr(),
            &mut privs.Privileges[0].Luid as *mut LUID,
        ) != 0
    };
    if !success {
        return Err(ShutdownError {
            kind: ShutdownErrorKind::LookupPrivilegeValue,
            source: get_last_error(),
        });
    }

    privs.PrivilegeCount = 1;
    privs.Privileges[0].Attributes = SE_PRIVILEGE_ENABLED;

    let success = unsafe {
        AdjustTokenPrivileges(
            token.as_ptr(),
            false as BOOL,
            &mut privs as *mut TOKEN_PRIVILEGES,
            0 as DWORD,
            null_mut::<TOKEN_PRIVILEGES>(),
            null_mut::<DWORD>(),
        ) != 0
    };

    if !success {
        return Err(ShutdownError {
            kind: ShutdownErrorKind::AdjustTokenPrivileges,
            source: get_last_error(),
        });
    }

    let reason = CString::new(reason).unwrap();
    let success = unsafe {
        InitiateSystemShutdownExA(
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
            SHTDN_REASON_MINOR_OTHER | SHTDN_REASON_FLAG_PLANNED,
        ) != 0
    };

    if !success {
        return Err(ShutdownError {
            kind: ShutdownErrorKind::InitiateSystemShutdown,
            source: get_last_error(),
        });
    }

    Ok(())
}
