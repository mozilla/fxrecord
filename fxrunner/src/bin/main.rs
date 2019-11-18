// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::time::Duration;

use libfxrecord::{run, CommonOptions};
use slog::{info, Logger};
use structopt::StructOpt;
use tokio::net::TcpListener;
use tokio::timer::delay_for;

use fxrunner::proto::RunnerProto;

use fxrunner::config::Config;

#[derive(Debug, StructOpt)]
#[structopt(name = "fxrunner", about = "Start FxRunner")]
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
    run::<Options, Config, _, _>(fxrunner, "fxrunner");
}

async fn fxrunner(log: Logger, _options: Options, config: Config) -> Result<(), Box<dyn Error>> {
    loop {
        let mut listener = TcpListener::bind(&config.host).await?;

        loop {
            let (stream, addr) = listener.accept().await?;
            info!(log, "Received connection"; "peer" => addr);
            let mut proto = RunnerProto::new(log.clone(), stream);

            let restart = proto.handshake_reply().await?;

            if restart {
                drop(proto);
                drop(listener);

                delay_for(Duration::from_secs(30)).await;
                info!(log, "\"Restarted\"");

                listener = TcpListener::bind(&config.host).await?;
            }
        }
    }
}
