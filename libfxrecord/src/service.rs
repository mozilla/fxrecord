// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

/// A RPC service for controlling an instance of FxRunner.
#[tarpc::service]
pub trait FxRunnerService {
    /// Request the runner to restart.
    ///
    /// Currently a no-op.
    async fn request_restart();
}
