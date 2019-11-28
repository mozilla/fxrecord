// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::error::Error;
use std::fmt::Debug;
use std::future::Future;
use std::path::Path;
use std::process::exit;

use futures::future::TryFutureExt;
use serde::Deserialize;
use slog::{error, info, Logger};
use structopt::StructOpt;
use tokio::runtime::Runtime;

use crate::config::read_config;
use crate::logging::build_logger;

pub mod config;
pub mod error;
pub mod logging;
pub mod net;
pub mod prefs;

/// A trait for exposing options common to both fxrunner and fxrecorder.
pub trait CommonOptions: StructOpt + Debug {
    /// The path to the `fxrecord.toml` file;
    fn config_path(&self) -> &Path;
}

/// A common main function that handles setting up logging and running `F` on a
/// tokio runtime.
pub fn run<O, C, F, Fut>(f: F, section: &'static str)
where
    O: CommonOptions,
    for<'de> C: Deserialize<'de>,
    Fut: Future<Output = Result<(), Box<dyn Error>>>,
    F: FnOnce(Logger, O, C) -> Fut,
{
    let options = O::from_args();
    let log = build_logger();

    info!(log, "read command-line options"; "options" => ?options);

    let result = read_config(options.config_path(), section)
        .map_err(|e| Box::new(e) as Box<dyn Error>)
        .and_then({
            let log = log.clone();
            move |config| {
                let mut rt = Runtime::new().expect("could not get tokio runtime");

                rt.block_on(f(log, options, config).map_err(Into::into))
            }
        });

    if let Err(e) = result {
        error!(log, "unexpected error"; "error" => %e);
        // We have to explicitly drop log here to flush output because
        // std::process::exit will not.
        drop(log);
        exit(1);
    }
}
