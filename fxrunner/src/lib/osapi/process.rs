// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

/// Abstractions for dealing with processes on Windows.
use std::convert::TryFrom;
use std::io;
use std::ptr::null;

use winapi::ctypes::c_void;
use winapi::shared::minwindef::{DWORD, UINT};
use winapi::shared::{minwindef, winerror};
use winapi::um::{handleapi, processsnapshot, processthreadsapi};

use crate::osapi::error::{check_nonzero, check_success};
use crate::osapi::handle::{Handle, ProcessSnapshot, ProcessSnapshotWalkMarker};

/// Open a handle to a process given by its process ID.
pub fn open_process(pid: DWORD, desired_access: DWORD) -> Result<Handle, io::Error> {
    Handle::try_from(unsafe {
        processthreadsapi::OpenProcess(desired_access, minwindef::FALSE, pid)
    })
}

/// Terminate the process that the handle points to.
///
/// The handle must have the `PROCESS_TERMINATE` permission.
pub fn terminate_process(process: &Handle, exit_status: UINT) -> Result<(), io::Error> {
    check_nonzero(unsafe { processthreadsapi::TerminateProcess(process.as_ptr(), exit_status) })
        .map(drop)
}

/// Iterate over the children of `process`.
///
/// Each process will be opened with permissions equal to the flags in
/// `desired_access`.
///
/// `process` must be opened with the `PROCESS_ALL_ACCESS` permission.
pub fn child_processes(
    process: Handle,
    desired_access: DWORD,
) -> Result<ChildProcessIter, io::Error> {
    ChildProcessIter::new(process, desired_access)
}

/// An iterator over child processes.
pub struct ChildProcessIter {
    /// The handle to the process that we are iterating.
    process_handle: Handle,
    /// The desired access to child processes.
    desired_access: DWORD,

    /// The handle to the process snapshot that we walk.
    snapshot: ProcessSnapshot,

    /// A walk maker for handle entries.
    walk_marker: ProcessSnapshotWalkMarker,

    /// A buffer to store the current handle entry in while iterating.
    buffer: detail::PSS_HANDLE_ENTRY,
}

impl ChildProcessIter {
    /// Create a new iterator for the children of `process_handle`.
    fn new(process_handle: Handle, desired_access: DWORD) -> Result<Self, io::Error> {
        let mut snapshot = ProcessSnapshot::null();

        // If we don't sleep here, the call to PssCaptureSnapshot() fails in
        // memcpy(), crashing us with a `STATUS_ACCESS_VIOLATION` error.
        //
        // If we sleep for too short of a duration (e.g., 1ms), this entire
        // function will intermittently block forever.
        std::thread::sleep(std::time::Duration::from_millis(500));

        check_success(unsafe {
            processsnapshot::PssCaptureSnapshot(
                process_handle.as_ptr(),
                processsnapshot::PSS_CAPTURE_HANDLES
                    | processsnapshot::PSS_CAPTURE_HANDLE_TYPE_SPECIFIC_INFORMATION,
                0,
                snapshot.as_out_ptr(),
            )
        })?;

        let mut walk_marker = ProcessSnapshotWalkMarker::null();
        check_success(unsafe {
            processsnapshot::PssWalkMarkerCreate(null(), walk_marker.as_out_ptr())
        })?;

        Ok(ChildProcessIter {
            process_handle,
            snapshot,
            walk_marker,
            desired_access,
            buffer: unsafe { std::mem::zeroed() },
        })
    }

    /// Attempt to return the next child process handle.
    fn try_next(&mut self) -> Result<Option<Handle>, io::Error> {
        loop {
            let rv = unsafe {
                processsnapshot::PssWalkSnapshot(
                    self.snapshot.as_ptr(),
                    processsnapshot::PSS_WALK_HANDLES,
                    self.walk_marker.as_ptr(),
                    &mut self.buffer as *mut detail::PSS_HANDLE_ENTRY as *mut c_void,
                    std::mem::size_of::<detail::PSS_HANDLE_ENTRY>() as u32,
                )
            };

            if rv == winerror::ERROR_NO_MORE_ITEMS {
                return Ok(None);
            } else if rv != winerror::ERROR_SUCCESS {
                return Err(io::Error::from_raw_os_error(rv as i32));
            }

            if self.buffer.ObjectType == detail::PSS_OBJECT_TYPE_PROCESS {
                let mut handle = Handle::null();

                // We cannot use the handle unless we duplicate it into our
                // process.
                check_nonzero(unsafe {
                    handleapi::DuplicateHandle(
                        self.process_handle.as_ptr(),
                        self.buffer.Handle,
                        processthreadsapi::GetCurrentProcess(),
                        handle.as_out_ptr(),
                        self.desired_access,
                        minwindef::FALSE,
                        0,
                    )
                })?;

                return Ok(Some(handle));
            }
        }
    }
}

impl Iterator for ChildProcessIter {
    type Item = Result<Handle, io::Error>;

    fn next(&mut self) -> Option<Self::Item> {
        self.try_next().transpose()
    }
}

mod detail {
    //! Types required for process snapshotting that are missing from winapi as
    //! of version 0.3.9.

    #![allow(dead_code)]
    #![allow(non_camel_case_types)]
    #![allow(non_snake_case)]

    use winapi::ctypes::{c_int, c_void, wchar_t};

    use winapi::shared::basetsd::ULONG_PTR;
    use winapi::shared::minwindef::{BOOL, DWORD, FILETIME, WORD};
    use winapi::shared::ntdef::{HANDLE, LARGE_INTEGER, LONG, ULONG};

    pub type PSS_HANDLE_FLAGS = u32;

    pub const PSS_HANDLE_NONE: PSS_HANDLE_FLAGS = 0;
    pub const PSS_HANDLE_HAVE_TYPE: PSS_HANDLE_FLAGS = 1;
    pub const PSS_HANDLE_HAVE_NAME: PSS_HANDLE_FLAGS = 2;
    pub const PSS_HANDLE_HAVE_BASIC_INFORMATION: PSS_HANDLE_FLAGS = 3;
    pub const PSS_HANDLE_HAVE_TYPE_SPECIFIC_INFORMATION: PSS_HANDLE_FLAGS = 4;

    pub type PSS_OBJECT_TYPE = u32;

    pub const PSS_OBJECT_TYPE_UNKNOWN: PSS_OBJECT_TYPE = 0;
    pub const PSS_OBJECT_TYPE_PROCESS: PSS_OBJECT_TYPE = 1;
    pub const PSS_OBJECT_TYPE_THREAD: PSS_OBJECT_TYPE = 2;
    pub const PSS_OBJECT_TYPE_MUTANT: PSS_OBJECT_TYPE = 3;
    pub const PSS_OBJECT_TYPE_EVENT: PSS_OBJECT_TYPE = 4;
    pub const PSS_OBJECT_TYPE_SECTION: PSS_OBJECT_TYPE = 5;
    pub const PSS_OBJECT_TYPE_SEMAPHORE: PSS_OBJECT_TYPE = 6;

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct PSS_HANDLE_ENTRY_Process {
        pub ExitStatus: DWORD,
        pub TebBaseAddress: *const c_void,
        pub AffinityMask: ULONG_PTR,
        pub BasePriority: ULONG,
        pub ProcessId: DWORD,
        pub ParentProcessId: DWORD,
        pub Flags: DWORD,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct PSS_HANDLE_ENTRY_Thread {
        pub ExitStatus: DWORD,
        pub TebBaseAddress: *const c_void,
        pub ProcessId: DWORD,
        pub ThreadId: DWORD,
        pub AffinityMask: ULONG_PTR,
        pub Priority: c_int,
        pub BasePriority: c_int,
        pub Win32StartAddress: *const c_void,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct PSS_HANDLE_ENTRY_Mutant {
        pub CurrentCount: LONG,
        pub Abandoned: BOOL,
        pub OwnerProcessId: DWORD,
        pub OwnerThreadId: DWORD,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct PSS_HANDLE_ENTRY_Event {
        ManualReset: BOOL,
        Signaled: BOOL,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct PSS_HANDLE_ENTRY_Section {
        pub BaseAddress: *const c_void,
        pub AllocationAttributes: DWORD,
        pub MaximumSize: LARGE_INTEGER,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct PSS_HANDLE_ENTRY_Semaphore {
        pub CurrentCount: LONG,
        pub MaximumCount: LONG,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub union PSS_HANDLE_ENTRY_TypeSpecificInformation {
        pub Process: PSS_HANDLE_ENTRY_Process,
        pub Thread: PSS_HANDLE_ENTRY_Thread,
        pub Mutant: PSS_HANDLE_ENTRY_Mutant,
        pub Event: PSS_HANDLE_ENTRY_Event,
        pub Section: PSS_HANDLE_ENTRY_Section,
        pub Semaphore: PSS_HANDLE_ENTRY_Semaphore,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct PSS_HANDLE_ENTRY {
        pub Handle: HANDLE,
        pub Flags: PSS_HANDLE_FLAGS,
        pub ObjectType: PSS_OBJECT_TYPE,
        pub CaptureTime: FILETIME,
        pub Attributes: DWORD,
        pub GrantedAccess: DWORD,
        pub HandleCount: DWORD,
        pub PointerCount: DWORD,
        pub PagedPoolCharge: DWORD,
        pub NonPagedPoolCharge: DWORD,
        pub CreationTime: FILETIME,
        pub TypeNameLength: WORD,
        pub TypeName: *const wchar_t,
        pub ObjectNameLength: WORD,
        pub ObjectName: *const wchar_t,
        pub TypeSpecificInformation: PSS_HANDLE_ENTRY_TypeSpecificInformation,
    }
}
