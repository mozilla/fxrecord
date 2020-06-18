// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use libfxrecord::error::ErrorMessage;
use libfxrunner::osapi::ShutdownProvider;

#[derive(Debug, Default)]
pub struct TestShutdownProvider {
    error: Option<&'static str>,
}

impl TestShutdownProvider {
    pub fn with_error(s: &'static str) -> Self {
        TestShutdownProvider { error: Some(s) }
    }
}

impl ShutdownProvider for TestShutdownProvider {
    type Error = ErrorMessage<&'static str>;

    fn initiate_restart(&self, _reason: &str) -> Result<(), Self::Error> {
        match self.error {
            Some(ref e) => Err(ErrorMessage(e)),
            None => Ok(()),
        }
    }
}
