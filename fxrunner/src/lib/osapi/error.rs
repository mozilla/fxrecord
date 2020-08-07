// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::ffi::CString;
use std::mem::forget;
use std::ptr::{null, null_mut};

use libfxrecord::error::ErrorMessage;
use thiserror::Error;
use winapi::shared::minwindef::{DWORD, HLOCAL};
use winapi::shared::ntdef::{self, LPSTR};
use winapi::um::{errhandlingapi, winbase};

/// An error from Windows.
///
/// The error will be formatted with `FormatMessageA` if possible.
#[derive(Debug, Error)]
#[error("{}", .0)]
pub struct WindowsError(WindowsErrorImpl);

#[derive(Debug, Error)]
enum WindowsErrorImpl {
    #[error(
        "An error occurred {:X}. Additionally, we could not format the error with FormatMessageA: {:X}.",
        .error_code,
        .format_error_code
    )]
    FormatMessageFailed {
        error_code: DWORD,
        format_error_code: DWORD,
    },

    #[error(transparent)]
    Message(ErrorMessage<String>),
}

/// Return the last windows error the occurred.
pub fn get_last_error() -> WindowsError {
    let error_code = unsafe { errhandlingapi::GetLastError() };
    let mut buf_ptr: LPSTR = null_mut();

    let rv = unsafe {
        winbase::FormatMessageA(
            winbase::FORMAT_MESSAGE_ALLOCATE_BUFFER
                | winbase::FORMAT_MESSAGE_FROM_SYSTEM
                | winbase::FORMAT_MESSAGE_IGNORE_INSERTS,
            null(),
            error_code,
            ntdef::MAKELANGID(ntdef::LANG_NEUTRAL, ntdef::SUBLANG_DEFAULT).into(),
            // When the FORMAT_MESSAGE_ALLOCATE_BUFFER flag is passed,
            // FormatMessageA treats this argument as a char** and will store
            // the pointer to the allocated buffer in `ptr`.
            //
            // See: https://docs.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-formatmessagea
            &mut buf_ptr as *mut LPSTR as LPSTR,
            0,
            null_mut(),
        )
    };

    if rv == 0 {
        // We couldn't get a description of the error.
        let format_error_code = unsafe { errhandlingapi::GetLastError() };
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
        winbase::LocalFree(buf_ptr as HLOCAL);
    }

    WindowsError(WindowsErrorImpl::Message(ErrorMessage(msg)))
}
