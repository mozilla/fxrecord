// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::time::Duration;

use libfxrecord::error::ErrorMessage;
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

    /// The ID of a build task that will be used by the runner.
    task_id: String,

    /// The path to a zipped Firefox profile for the runner to use.
    ///
    /// If not provided, the runner will create a new profile.
    #[structopt(long = "profile")]
    profile_path: Option<PathBuf>,
}

impl CommonOptions for Options {
    fn config_path(&self) -> &Path {
        &self.config_path
    }
}

fn main() {
    run::<Options, Config, _, _>(fxrecorder, "fxrecorder");
}

async fn fxrecorder(log: Logger, options: Options, config: Config) -> Result<(), Box<dyn Error>> {
    if let Some(ref profile_path) = options.profile_path {
        let meta = tokio::fs::metadata(profile_path).await?;

        if !meta.is_file() {
            return Err(ErrorMessage("profile is not a file").into());
        }
    }

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
        proto.download_build(&options.task_id).await?;
        proto
            .send_profile(options.profile_path.as_ref().map(PathBuf::as_path))
            .await?;
    }

    Ok(())
}
