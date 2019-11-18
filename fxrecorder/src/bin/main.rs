// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::time::Duration;

use libfxrecord::{run, CommonOptions};
use slog::{error, info, Logger};
use structopt::StructOpt;
use tokio::net::TcpStream;

use fxrecorder::config::Config;
use fxrecorder::proto::RecorderProto;
use fxrecorder::retry::exponential_retry;

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
    {
        let stream = TcpStream::connect(&config.host).await?;
        info!(log, "Connected"; "peer" => config.host);

        let mut proto = RecorderProto::new(log.clone(), stream);

        proto.handshake(true).await?;
    }

    {
        let reconnect = || {
            info!(log, "Attempting re-connection to runner...");
            TcpStream::connect(&config.host)
        };

        // 30 * 2^4 = 7:30
        let stream = exponential_retry(reconnect, Duration::from_secs(30), 4)
            .await
            .map_err(|e| {
                error!(
                    log,
                    "Could not connect to runner";
                    "last_error" => ?e.source().unwrap()
                );
                e
            })?;

        info!(log, "Re-connected"; "peer" => config.host);

        let mut proto = RecorderProto::new(log, stream);

        proto.handshake(false).await?;
    }

    Ok(())
}
