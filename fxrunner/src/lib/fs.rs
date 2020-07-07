// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use async_trait::async_trait;
use tokio::fs::metadata;

/// An extension trait for Path to add async versions of its metadata helpers.
#[async_trait]
pub trait PathExt {
    /// An async version of `std::path::Path::is_file`.
    async fn is_file_async(&self) -> bool;

    /// An async version of `std::path::Path::is_dir`.
    async fn is_dir_async(&self) -> bool;
}

#[async_trait]
impl PathExt for std::path::Path {
    async fn is_file_async(&self) -> bool {
        match metadata(self).await {
            Ok(m) => m.is_file(),
            Err(..) => false,
        }
    }

    async fn is_dir_async(&self) -> bool {
        match metadata(self).await {
            Ok(m) => m.is_dir(),
            Err(..) => false,
        }
    }
}
