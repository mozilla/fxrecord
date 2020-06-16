// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Traits for interacting safely with OS-level APIs.

mod error;
mod handle;
mod shutdown;

/// A trait providing the ability to restart the current machine.
pub trait ShutdownProvider {
    /// The error
    type Error: std::error::Error + 'static;

    /// Initiate a restart with the given reason.
    fn initiate_restart(&self, reason: &str) -> Result<(), Self::Error>;
}

/// A [`ShutdownProvider`](trait.ShutdownProvider.html) that uses the Windows API.
#[derive(Default)]
pub struct WindowsShutdownProvider {
    /// Whether or not to skip the actual restart.
    #[cfg(debug_assertions)]
    skip_restart: bool,
}

#[cfg(debug_assertions)]
impl WindowsShutdownProvider {
    pub fn skipping_restart(skip_restart: bool) -> Self {
        let mut provider = WindowsShutdownProvider::default();
        provider.skip_restart = skip_restart;
        provider
    }
}

impl ShutdownProvider for WindowsShutdownProvider {
    type Error = shutdown::ShutdownError;

    #[cfg(debug_assertions)]
    fn initiate_restart(&self, reason: &str) -> Result<(), Self::Error> {
        if self.skip_restart {
            Ok(())
        } else {
            shutdown::initiate_restart(reason)
        }
    }

    #[cfg(not(debug_assertions))]
    fn initiate_restart(&self, reason: &str) -> Result<(), Self::Error> {
        shutdown::initiate_restart(reason)
    }
}
