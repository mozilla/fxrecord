// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::ffi::c_void;
use std::ptr::null_mut;

use winapi::um::handleapi;
use winapi::um::winnt::HANDLE;

/// A HANDLE that is closed when dropped.
pub type Handle = AutoClosingHandle<c_void>;

pub struct AutoClosingHandle<T>(*mut T)
where
    *mut T: ClosableHandle;

/// A trait representing a raw resource handle that can be closed.
pub trait ClosableHandle {
    /// Close the handle.
    fn close(&mut self);
}

impl ClosableHandle for HANDLE {
    fn close(&mut self) {
        if !self.is_null() {
            let rv = unsafe { handleapi::CloseHandle(*self) };
            assert!(rv != 0);
        }
    }
}

impl<T> Drop for AutoClosingHandle<T>
where
    *mut T: ClosableHandle,
{
    fn drop(&mut self) {
        self.0.close()
    }
}

impl<T> AutoClosingHandle<T>
where
    *mut T: ClosableHandle,
{
    /// Create a new handle.
    pub fn null() -> Self {
        Self(null_mut())
    }

    /// Return the underlying handle.
    pub fn as_ptr(&self) -> *mut T {
        self.0
    }

    /// Return a mutable double pointer to the underlying handle, allowing it to
    /// be used as an output parameter.
    pub fn as_out_ptr(&mut self) -> *mut *mut T {
        &mut self.0 as *mut *mut T
    }
}

impl<T> From<*mut T> for AutoClosingHandle<T>
where
    *mut T: ClosableHandle,
{
    fn from(handle: *mut T) -> Self {
        Self(handle)
    }
}
