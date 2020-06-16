// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::ptr::null_mut;

use winapi::um::handleapi::CloseHandle;
use winapi::um::winnt::HANDLE;

// A wrapper around a HANDLE that automatically closes.
pub struct Handle(HANDLE);

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
            let rv = unsafe { CloseHandle(self.0) };
            assert!(rv != 0);

            self.0 = null_mut();
        }
    }
}
