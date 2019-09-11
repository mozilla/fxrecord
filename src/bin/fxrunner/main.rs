// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

mod config;

use std::error;
use std::path::PathBuf;
use std::process::exit;

use derive_more::Display;
use slog::{error, info, Logger};
use structopt::StructOpt;

use crate::config::Config;
use fxrecord::config::{read_config, ConfigError};
use fxrecord::logging::build_logger;

#[derive(Debug, StructOpt)]
#[structopt(name = "fxrunner", about = "Start FxRunner")]
struct Options {
    /// The configuration file to use.
    #[structopt(long = "config", default_value = "fxrecord.toml")]
    config_path: PathBuf,
}

fn main() {
    let options = Options::from_args();
    let log = build_logger();

    if let Err(e) = fxrunner(log.clone(), options) {
        error!(log, "unexpected error"; "error" => %e);
        drop(log);
        exit(1);
    }
}

/// An error that occurred in FxRunner.
#[derive(Debug, Display)]
enum Error {
    #[display(fmt = "{}", _0)]
    Config(ConfigError),
}

impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match &self {
            Error::Config(ref e) => Some(e),
        }
    }
}

fn fxrunner(log: Logger, options: Options) -> Result<(), Error> {
    info!(log, "read command-line options"; "options" => ?options);

    let config: Config = read_config(&options.config_path, "fxrunner").map_err(Error::Config)?;
    info!(log, "read config"; "config" => ?config);

    Ok(())
}