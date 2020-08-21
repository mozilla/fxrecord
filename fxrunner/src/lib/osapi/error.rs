// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::io;

use num_traits::Zero;
use winapi::shared::minwindef::DWORD;
use winapi::shared::winerror;

/// Check the result of a Windows API call that returns nonzero on success.
///
/// ```
/// # use std::io;
/// #
/// # use winapi::shared::minwindef::BOOL;
/// # use winapi::um::errhandlingapi;
/// # use winapi::shared::winerror;
/// #
/// # use libfxrunner::osapi::error::check_nonzero;
/// unsafe fn succeeds() -> BOOL {
///   errhandlingapi::SetLastError(winerror::ERROR_SUCCESS);
///   1
/// }
///
/// assert!(check_nonzero(unsafe { succeeds() }).is_ok());
///
/// unsafe fn fails() -> BOOL {
///   errhandlingapi::SetLastError(winerror::ERROR_ACCESS_DENIED);
///   0
/// }
///
/// assert_eq!(
///   check_nonzero(unsafe { fails() }).unwrap_err().raw_os_error().unwrap(),
///   winerror::ERROR_ACCESS_DENIED as i32,
/// );
/// ```
pub fn check_nonzero<T>(val: T) -> Result<T, io::Error>
where
    T: Clone + Copy + Eq + Zero,
{
    if val != Zero::zero() {
        Ok(val)
    } else {
        Err(io::Error::last_os_error())
    }
}

/// Check the result of a Windows API call that returns a pointer to an object.
///
/// If that pointer is null, return the last error that occurred.
///
/// ```
/// # use std::io;
/// # use std::ffi::c_void;
/// # use std::ptr::null_mut;
/// #
/// # use winapi::shared::winerror;
/// # use winapi::um::errhandlingapi;
/// #
/// # use libfxrunner::osapi::error::check_nonnull;
/// unsafe fn succeeds() -> *mut u32 {
///   errhandlingapi::SetLastError(winerror::ERROR_SUCCESS);
///   0xFFFFFFFFFFFFFFFF as *mut u32
/// }
///
/// assert_eq!(
///   check_nonnull(unsafe { succeeds() }).unwrap(),
///   0xFFFFFFFFFFFFFFFF as *mut u32
///  );
///
/// unsafe fn fails() -> *mut u32 {
///   errhandlingapi::SetLastError(winerror::ERROR_ACCESS_DENIED);
///   null_mut()
/// }
///
/// assert_eq!(
///   check_nonnull(unsafe { fails() }).unwrap_err().raw_os_error().unwrap(),
///   winerror::ERROR_ACCESS_DENIED as i32,
/// );
/// ```
pub fn check_nonnull<T>(ptr: *mut T) -> Result<*mut T, io::Error> {
    if !ptr.is_null() {
        Ok(ptr)
    } else {
        Err(io::Error::last_os_error())
    }
}

/// Check the result of a Windows API call that returns an error code.
///
/// ```
/// # use std::io;
/// #
/// # use winapi::shared::minwindef::DWORD;
/// # use winapi::shared::winerror;
/// #
/// # use libfxrunner::osapi::error::check_success;
/// fn succeeds() -> DWORD { winerror::ERROR_SUCCESS }
/// fn fails() -> DWORD { winerror::ERROR_ACCESS_DENIED }
///
/// assert!(check_success(succeeds()).is_ok());
///
/// assert_eq!(
///   check_success(fails()).unwrap_err().raw_os_error().unwrap(),
///   winerror::ERROR_ACCESS_DENIED as i32,
/// );
/// ```
pub fn check_success(err: DWORD) -> Result<(), io::Error> {
    if err == winerror::ERROR_SUCCESS {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(err as i32))
    }
}
