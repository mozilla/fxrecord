// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::error::Error;
use std::ffi::CString;
use std::mem::forget;
use std::os::raw::c_void;
use std::ptr::{null, null_mut};

use derive_more::Display;
use libfxrecord::error::ErrorMessage;
use winapi::shared::minwindef::{BOOL, DWORD, HLOCAL};
use winapi::shared::ntdef::{LANG_NEUTRAL, LPSTR, LUID, MAKELANGID, SUBLANG_DEFAULT};
use winapi::um::errhandlingapi::GetLastError;
use winapi::um::processthreadsapi::{GetCurrentProcess, OpenProcessToken};
use winapi::um::reason::SHTDN_REASON_MINOR_OTHER;
use winapi::um::securitybaseapi::AdjustTokenPrivileges;
use winapi::um::winbase::{
    FormatMessageA, LocalFree, LookupPrivilegeValueA, FORMAT_MESSAGE_ALLOCATE_BUFFER,
    FORMAT_MESSAGE_FROM_SYSTEM, FORMAT_MESSAGE_IGNORE_INSERTS,
};
use winapi::um::winnt::{
    HANDLE, SE_PRIVILEGE_ENABLED, SE_SHUTDOWN_NAME, TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES,
    TOKEN_QUERY,
};
use winapi::um::winreg::InitiateSystemShutdownExA;

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
    source: ErrorMessage<String>,
}

impl Error for ShutdownError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.source)
    }
}

// See: https://docs.microsoft.com/en-us/windows/win32/shutdown/how-to-shut-down-the-system
pub fn initiate_restart(reason: &str) -> Result<(), ShutdownError> {
    let mut token: HANDLE = null_mut::<c_void>();
    let mut privs = unsafe { std::mem::zeroed::<TOKEN_PRIVILEGES>() };

    let name = CString::new(SE_SHUTDOWN_NAME).unwrap();
    let success = unsafe {
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
            &mut token as *mut HANDLE,
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
            token,
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

    let reason_ptr = CString::new(reason).unwrap().into_raw();
    let success = unsafe {
        let rv = InitiateSystemShutdownExA(
            null_mut(),
            reason_ptr as LPSTR,
            10,
            true as BOOL,
            true as BOOL,
            SHTDN_REASON_MINOR_OTHER,
        );

        // De-allocate the C string.
        CString::from_raw(reason_ptr);

        rv != 0
    };

    if !success {
        return Err(ShutdownError {
            kind: ShutdownErrorKind::InitiateSystemShutdown,
            source: get_last_error(),
        });
    }

    Ok(())
}

/// Return the last windows error the occurred.
fn get_last_error() -> ErrorMessage<String> {
    let msg = unsafe {
        let error_code = GetLastError();
        let mut buf_ptr: LPSTR = null_mut();

        FormatMessageA(
            FORMAT_MESSAGE_ALLOCATE_BUFFER
                | FORMAT_MESSAGE_FROM_SYSTEM
                | FORMAT_MESSAGE_IGNORE_INSERTS,
            null(),
            error_code,
            MAKELANGID(LANG_NEUTRAL, SUBLANG_DEFAULT).into(),
            // When the FORMAT_MESSAGE_ALLOCATE_BUFFER flag is passed,
            // FormatMessageA treats this argument as a char** and will store
            // the pointer to the allocated buffer in `ptr`.
            //
            // See: https://docs.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-formatmessagea
            &mut buf_ptr as *mut LPSTR as LPSTR,
            0,
            null_mut::<LPSTR>(),
        );

        assert!(!buf_ptr.is_null());

        let cstr = CString::from_raw(buf_ptr);
        let s = cstr.to_string_lossy().into_owned();

        // We forget `cstr` here because we need to free `buf_ptr` with
        // `LocalFree`.
        forget(cstr);

        LocalFree(buf_ptr as HLOCAL);

        s
    };

    ErrorMessage(msg)
}
