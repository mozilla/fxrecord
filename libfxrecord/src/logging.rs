// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use slog::{Drain, Logger};

/// Create a logger.
pub fn build_logger() -> Logger {
    let decorator = slog_term::PlainDecorator::new(std::io::stdout());
    let drain = slog_term::FullFormat::new(decorator)
        .use_original_order()
        .use_utc_timestamp()
        .build()
        .fuse();

    let drain = slog_async::Async::new(drain).build().fuse();

    Logger::root(drain, slog::o! {})
}
