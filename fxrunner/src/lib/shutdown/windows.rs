// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::error::Error;
use std::ffi::CString;
use std::mem::forget;
use std::ptr::{null, null_mut};

use derive_more::Display;
use libfxrecord::error::ErrorMessage;
use winapi::shared::minwindef::{BOOL, DWORD, HLOCAL};
use winapi::shared::ntdef::{LANG_NEUTRAL, LPSTR, LUID, MAKELANGID, SUBLANG_DEFAULT};
use winapi::um::errhandlingapi::GetLastError;
use winapi::um::handleapi::CloseHandle;
use winapi::um::processthreadsapi::{GetCurrentProcess, OpenProcessToken};
use winapi::um::reason::{SHTDN_REASON_FLAG_PLANNED, SHTDN_REASON_MINOR_OTHER};
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
    source: WindowsError,
}

impl Error for ShutdownError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.source)
    }
}

/// A wrapper around a HANDLE that automatically closes.
struct Handle(HANDLE);

impl Handle {
    /// Create a new null handle.
    pub fn null() -> Self {
        Handle(null_mut())
    }

    /// Return the underlying `HANDLE`.
    pub fn as_ptr(&self) -> HANDLE {
        self.0
    }

    /// Return a mutable pointer to the underlying `HANDLE`, allowing it to be
    /// used as an output parameter.
    pub fn as_out_ptr(&mut self) -> *mut HANDLE {
        &mut self.0 as *mut HANDLE
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            let rv = unsafe { CloseHandle(self.0 as HANDLE) };
            assert!(rv != 0);

            self.0 = null_mut();
        }
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

/// An error from Windows.
///
/// The error will be formatted with `FormatMessageA` if possible.
#[derive(Debug, Display)]
pub struct WindowsError(WindowsErrorImpl);

impl Error for WindowsError {}

#[derive(Debug, Display)]
enum WindowsErrorImpl {
    #[display(
        fmt = "An error occurred {:X}. Additionally, we could not format the error with FormatMessageA: {:X}.",
        error_code,
        format_error_code
    )]
    FormatMessageFailed {
        error_code: DWORD,
        format_error_code: DWORD,
    },

    Message(ErrorMessage<String>),
}

/// Return the last windows error the occurred.
fn get_last_error() -> WindowsError {
    let error_code = unsafe { GetLastError() };
    let mut buf_ptr: LPSTR = null_mut();

    let rv = unsafe {
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
        )
    };

    if rv == 0 {
        // We couldn't get a description of the error.
        let format_error_code = unsafe { GetLastError() };
        return WindowsError(WindowsErrorImpl::FormatMessageFailed {
            error_code,
            format_error_code,
        });
    }

    assert!(!buf_ptr.is_null());

    let cstr = unsafe { CString::from_raw(buf_ptr) };
    let msg = cstr.to_string_lossy().into_owned();

    // We forget `cstr` here because we need to free `buf_ptr` with
    // `LocalFree`.
    forget(cstr);

    unsafe {
        LocalFree(buf_ptr as HLOCAL);
    }

    WindowsError(WindowsErrorImpl::Message(ErrorMessage(msg)))
}
