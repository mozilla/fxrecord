// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::error::Error;
use std::fmt::{Debug, Display};

use derive_more::Display;
use serde::{Deserialize, Serialize};

/// An error that consists only of a message and no state.
///
/// This struct is templated over anything displayable (instead of just using a
/// `String`) so that we can use type like `&'static str` as error messages and
/// avoid allocation.
#[derive(Debug, Deserialize, Display, Serialize)]
pub struct ErrorMessage<D: Debug + Display + Send + Sync + 'static>(pub D);

impl<D: Debug + Display + Send + Sync + 'static> Error for ErrorMessage<D> {}

/// An extension trait for `Error` that can convert errors into error messages
/// for transport across the network.
pub trait ErrorExt: Error {
    /// Convert the `Error` into an [`ErrorMessage`](struct.ErrorMessage.html).
    fn into_error_message(&self) -> ErrorMessage<String> {
        ErrorMessage(self.to_string())
    }
}

impl<E: Error> ErrorExt for E {}
