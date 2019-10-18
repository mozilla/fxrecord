// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

mod config;

use std::error::Error;
use std::path::{Path, PathBuf};

use slog::{info, Logger};
use structopt::StructOpt;

use crate::config::Config;
use fxrecord::{run, CommonOptions};

#[derive(Debug, StructOpt)]
#[structopt(name = "fxrunner", about = "Start FxRunner")]
struct Options {
    /// The configuration file to use.
    #[structopt(long = "config", default_value = "fxrecord.toml")]
    config_path: PathBuf,
}

impl CommonOptions for Options {
    fn config_path(&self) -> &Path {
        return &self.config_path;
    }
}

fn main() {
    run::<Options, Config, _>(fxrunner, "fxrunner");
}

fn fxrunner(log: Logger, _options: Options, _config: Config) -> Result<(), Box<dyn Error>> {
    Ok(())
}
