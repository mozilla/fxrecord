// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::ffi::CStr;
use std::io;
use std::ptr::{null, null_mut};
use std::thread;

use async_trait::async_trait;
use lazy_static::lazy_static;
use libfxrecord::ORANGE;
use tokio::sync::oneshot;
use winapi::shared::minwindef::{DWORD, HINSTANCE, LPARAM, LRESULT, UINT, WPARAM};
use winapi::shared::windef::HWND;
use winapi::shared::winerror;
use winapi::um::winuser::{MSG, WNDCLASSA};
use winapi::um::{libloaderapi, processthreadsapi, wingdi, winuser};

use crate::osapi::error::{check_nonnull, check_nonzero};

lazy_static! {
    static ref WINDOW_CLASS_NAME: &'static CStr =
        CStr::from_bytes_with_nul(&b"fxrunnerbg\0"[..]).unwrap();
}

const MESSAGE_CLOSE_SPLASH: UINT = winuser::WM_USER + 1;

#[async_trait]
pub trait Splash: Sized {
    async fn new(display_widht: u32, display_height: u32) -> Result<Self, io::Error>;
    fn destroy(&mut self) -> Result<(), io::Error>;
}

/// A splash screen that covers the entire display.
///
/// The splash screen is painted a solid red (#FF0000) so that the Firefox Window
/// can be easily differentiated from the background.
pub struct WindowsSplash {
    /// The thread ID of the UI thread, so that we may send messages to it via
    /// `PostThreadMessageA` API.
    ui_thread_id: DWORD,

    /// The join handle for the thread.
    ui_thread_join_handle: Option<thread::JoinHandle<()>>,
}

//
#[async_trait]
impl Splash for WindowsSplash {
    /// Create a new `Splash` with the given width and height.
    async fn new(display_width: u32, display_height: u32) -> Result<WindowsSplash, io::Error> {
        // We need to receive the result of window creation over a channel
        // because a window's event loop must run on the same thread that the
        // window was created on.
        //
        // We dedicate a background thread to just running the event loop. To
        // communicate with this thread, we can use
        // `winuser::PostThreadMessageA` to post a message to the event loop.
        let (tx, rx) = oneshot::channel::<Result<DWORD, io::Error>>();

        let join_handle = thread::spawn(move || {
            let window_handle = match create_and_show_window(display_width, display_height) {
                Ok(handle) => handle,
                Err(e) => {
                    tx.send(Err(e)).unwrap();
                    return;
                }
            };

            let thread_id = unsafe { processthreadsapi::GetCurrentThreadId() };
            tx.send(Ok(thread_id)).unwrap();

            run_message_loop(window_handle);
        });

        let thread_id = match rx.await.unwrap() {
            Ok(thread_id) => thread_id,
            Err(e) => {
                join_handle.join().unwrap();
                return Err(e);
            }
        };

        Ok(WindowsSplash {
            ui_thread_id: thread_id,
            ui_thread_join_handle: Some(join_handle),
        })
    }

    /// Destroy the `Splash` window.
    fn destroy(&mut self) -> Result<(), io::Error> {
        check_nonzero(unsafe {
            winuser::PostThreadMessageA(self.ui_thread_id, MESSAGE_CLOSE_SPLASH, 0, 0)
        })
        .map(drop)?;

        self.ui_thread_join_handle
            .take()
            .expect("Splash::destroy called without UI thread")
            .join()
            .expect("UI thread panicked");

        Ok(())
    }
}

impl Drop for WindowsSplash {
    fn drop(&mut self) {
        assert!(
            self.ui_thread_join_handle.is_none(),
            "Splash dropped without calling destroy()"
        );
    }
}

/// Register the window class that `Splash` will use.
///
/// It will only attempt to register the window class if it has not yet been
/// registered.
fn ensure_window_class_registered(instance: HINSTANCE) -> Result<(), io::Error> {
    let mut cls = WNDCLASSA::default();

    let exists = {
        let rv = unsafe {
            winuser::GetClassInfoA(
                instance,
                WINDOW_CLASS_NAME.as_ptr(),
                &mut cls as *mut WNDCLASSA,
            )
        };

        if rv == 0 {
            let err = io::Error::last_os_error();
            if err.raw_os_error().unwrap() != winerror::ERROR_CLASS_DOES_NOT_EXIST as i32 {
                return Err(err);
            }
            false
        } else {
            true
        }
    };

    if exists {
        return Ok(());
    }

    // This handle does not need to be freed.
    let brush = check_nonnull(unsafe {
        wingdi::CreateSolidBrush(wingdi::RGB(ORANGE[0], ORANGE[1], ORANGE[2]))
    })?;

    // This handle does not need to be freed.
    let cursor = check_nonnull(unsafe { winuser::LoadCursorW(null_mut(), winuser::IDC_ARROW) })?;

    cls.style = 0;
    cls.lpfnWndProc = Some(window_proc);
    cls.cbClsExtra = 0;
    cls.cbWndExtra = 0;
    cls.hInstance = instance;
    cls.hIcon = null_mut();
    cls.hCursor = cursor;
    cls.hbrBackground = brush;
    cls.lpszMenuName = null_mut();
    cls.lpszClassName = WINDOW_CLASS_NAME.as_ptr();

    check_nonzero(unsafe { winuser::RegisterClassA(&cls as *const WNDCLASSA) }).map(drop)
}

/// Create and show a window of the given size.
fn create_and_show_window(display_width: u32, display_height: u32) -> Result<HWND, io::Error> {
    let instance = check_nonnull(unsafe { libloaderapi::GetModuleHandleA(null()) })?;

    ensure_window_class_registered(instance)?;

    let window_handle = check_nonnull(unsafe {
        winuser::CreateWindowExA(
            winuser::WS_EX_NOACTIVATE,
            WINDOW_CLASS_NAME.as_ptr(),
            // We re-use the class name as the window name. There is no
            // title bar, so it is not displayed on screen.
            WINDOW_CLASS_NAME.as_ptr(),
            winuser::WS_MAXIMIZE | winuser::WS_POPUPWINDOW | winuser::WS_VISIBLE,
            0,
            0,
            display_width as i32,
            display_height as i32,
            null_mut(), // No parent window.
            null_mut(), // No menu.
            instance,
            null_mut(),
        )
    })?;

    Ok(window_handle)
}

/// Run the message loop for the window.
fn run_message_loop(window_handle: HWND) {
    let mut msg = MSG::default();
    loop {
        let rv = unsafe { winuser::GetMessageA(&mut msg as *mut MSG, null_mut(), 0, 0) };
        if rv <= 0 {
            // We received WM_QUIT, which means that our window proc has handled WM_DESTROY.
            return;
        } else if msg.message == MESSAGE_CLOSE_SPLASH {
            assert_ne!(
                unsafe { winuser::PostMessageA(window_handle, winuser::WM_CLOSE, 0, 0) },
                0
            );
        } else {
            unsafe {
                winuser::TranslateMessage(&msg as *const MSG);
                winuser::DispatchMessageA(&msg as *const MSG);
            }
        }
    }
}

unsafe extern "system" fn window_proc(
    window_handle: HWND,
    msg: UINT,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        winuser::WM_CLOSE => {
            winuser::DestroyWindow(window_handle);
            0
        }
        winuser::WM_DESTROY => {
            winuser::PostQuitMessage(0);
            0
        }
        _ => winuser::DefWindowProcA(window_handle, msg, wparam, lparam),
    }
}
