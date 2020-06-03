// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

mod config;

use std::error::Error;
use std::path::{Path, PathBuf};

use libfxrecord::{run, CommonOptions};
use slog::Logger;
use structopt::StructOpt;

use crate::config::Config;

#[derive(Debug, StructOpt)]
#[structopt(name = "fxrecorder", about = "Start FxRecorder")]
struct Options {
    /// The configuration file to use.
    #[structopt(long = "config", default_value = "fxrecord.toml")]
    config_path: PathBuf,
}

impl CommonOptions for Options {
    fn config_path(&self) -> &Path {
        &self.config_path
    }
}

fn main() {
    run::<Options, Config, _, _>(fxrecorder, "fxrecorder");
}

async fn fxrecorder(
    _log: Logger,
    _options: Options,
    _config: Config,
) -> Result<(), Box<dyn Error>> {
    Ok(())
}
