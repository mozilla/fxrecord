// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::error::Error;
use std::future::Future;
use std::time::Duration;

use thiserror::Error;
use tokio::time::delay_for;

#[derive(Debug, Error)]
#[error("failed after {} retries", retries)]
/// An error that occurred when retrying a fallable operation.
pub struct RetryError<E: Error + 'static> {
    /// The last error that occurred.
    source: E,

    /// The number of retries.
    retries: u32,
}

/// Attempt to resolve the future returned by the given function `retries` times
/// using exponential backoff before the first attempt and between subsequent
/// attempts.
pub async fn delayed_exponential_retry<F, Fut, T, E>(
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
