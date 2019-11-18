// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

mod windows;

/// A trait providing the ability to restart the current machine.
pub trait ShutdownProvider {
    /// The error
    type Error: std::error::Error + 'static;

    /// Initiate a restart with the given reason.
    fn initiate_restart(&self, reason: &str) -> Result<(), Self::Error>;
}

/// A [`ShutdownProvider`](trait.ShutdownProvider.html) that uses the Windows API.
#[derive(Default)]
pub struct WindowsShutdownProvider;

impl ShutdownProvider for WindowsShutdownProvider {
    type Error = windows::ShutdownError;

    fn initiate_restart(&self, reason: &str) -> Result<(), Self::Error> {
        windows::initiate_restart(reason)
    }
}
