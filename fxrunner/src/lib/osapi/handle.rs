// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::convert::TryFrom;
use std::ffi::c_void;
use std::io;
use std::ptr::null_mut;

use winapi::shared::winerror;
use winapi::um::processsnapshot::{HPSS, HPSSWALK, HPSSWALK__, HPSS__};
use winapi::um::winnt::HANDLE;
use winapi::um::{handleapi, processsnapshot, processthreadsapi};

/// A HANDLE that is closed when dropped.
pub type Handle = AutoClosingHandle<c_void>;

/// A HPSS that is freed when dropped.
///
/// It is only safe to use for snapshots captured by the current process.
pub type ProcessSnapshot = AutoClosingHandle<HPSS__>;
pub type ProcessSnapshotWalkMarker = AutoClosingHandle<HPSSWALK__>;

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

impl ClosableHandle for HPSS {
    fn close(&mut self) {
        if !self.is_null() {
            let rv = unsafe {
                processsnapshot::PssFreeSnapshot(processthreadsapi::GetCurrentProcess(), *self)
            };
            assert!(rv == winerror::ERROR_SUCCESS);
        }
    }
}

impl ClosableHandle for HPSSWALK {
    fn close(&mut self) {
        if !self.is_null() {
            let rv = unsafe { processsnapshot::PssWalkMarkerFree(*self) };
            assert!(rv == winerror::ERROR_SUCCESS);
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

impl TryFrom<HANDLE> for Handle {
    type Error = io::Error;

    fn try_from(h: HANDLE) -> Result<Self, Self::Error> {
        if h == handleapi::INVALID_HANDLE_VALUE {
            Err(io::Error::last_os_error())
        } else {
            Ok(AutoClosingHandle(h))
        }
    }
}
