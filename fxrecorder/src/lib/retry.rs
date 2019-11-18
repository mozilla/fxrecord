// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::error::Error;
use std::future::Future;
use std::time::Duration;

use derive_more::Display;
use tokio::timer::delay_for;

#[derive(Debug, Display)]
#[display(fmt = "failed after {} retries", retries)]
/// An error that occurred when retrying a fallable operation.
pub struct RetryError<E: Error + 'static> {
    /// The last error that occurred.
    source: E,

    /// The number of retries.
    retries: u32,
}

impl<E: Error + 'static> Error for RetryError<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.source)
    }
}

/// Attempt to resolve the future returned by the given function `retries` times
/// using exponential backoff between attempts.
pub async fn exponential_retry<F, Fut, T, E>(
    f: F,
    wait: Duration,
    retries: u32,
) -> Result<T, RetryError<E>>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T, E>>,
    E: Error + 'static,
{
    let mut t = wait;
    let mut last_error = None;

    for _ in 0..retries {
        delay_for(t).await;

        match f().await {
            Ok(r) => return Ok(r),
            Err(e) => last_error = Some(e),
        }

        t *= 2;
    }

    Err(RetryError {
        source: last_error.unwrap(),
        retries,
    })
}
