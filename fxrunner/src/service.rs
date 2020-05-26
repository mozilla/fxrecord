// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use futures::future;
use futures::future::Ready;
use libfxrecord::service::FxRunnerService;
use tarpc::context::Context;

#[derive(Clone)]
pub struct FxRunner;

impl FxRunnerService for FxRunner {
    type RequestRestartFut = Ready<()>;

    fn request_restart(self, _: Context) -> Self::RequestRestartFut {
        future::ready(())
    }
}
