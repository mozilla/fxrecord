// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::error::Error;

mod windows;

pub trait Shutdown {
    type Error: Error + 'static;

    fn initiate_restart(&self, reason: &str) -> Result<(), Self::Error>;
}

pub struct WindowsShutdown;
impl Shutdown for WindowsShutdown {
    type Error = windows::ShutdownError;

    fn initiate_restart(&self, reason: &str) -> Result<(), Self::Error> {
        windows::initiate_restart(reason)
    }
}

#[cfg(debug_assertions)]
mod debug {
    use super::{windows, Shutdown};

    pub struct DebugShutdown {
        pub skip_restart: bool,
    }

    impl Shutdown for DebugShutdown {
        type Error = windows::ShutdownError;

        fn initiate_restart(&self, reason: &str) -> Result<(), Self::Error> {
            if self.skip_restart {
                Ok(())
            } else {
                windows::initiate_restart(reason)
            }
        }
    }
}

#[cfg(debug_assertions)]
pub use debug::DebugShutdown;
