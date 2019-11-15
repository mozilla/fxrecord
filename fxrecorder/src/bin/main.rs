// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::error::Error;
use std::path::{Path, PathBuf};

use libfxrecord::{run, CommonOptions};
use libfxrecorder::config::Config;
use slog::{info, Logger};
use structopt::StructOpt;

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

async fn fxrecorder(log: Logger, _options: Options, config: Config) -> Result<(), Box<dyn Error>> {
    use libfxrecorder::proto::RecorderProto;
    use tokio::net::TcpStream;

    let stream = TcpStream::connect(&config.host).await?;
    info!(log, "Connected"; "peer" => config.host);

    let mut proto = RecorderProto::new(log, stream);

    proto.handshake().await?;

    Ok(())
}