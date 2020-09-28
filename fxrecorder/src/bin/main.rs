// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::error::Error;
use std::path::PathBuf;
use std::process::exit;
use std::time::Duration;

use libfxrecord::config::read_config;
use libfxrecord::error::ErrorMessage;
use libfxrecord::logging::build_logger;
use libfxrecord::net::Idle;
use libfxrecord::prefs::{parse_pref, PrefValue};
use libfxrecorder::config::Config;
use libfxrecorder::proto::RecorderProto;
use libfxrecorder::recorder::FfmpegRecorder;
use libfxrecorder::retry::delayed_exponential_retry;
use slog::{error, info, Logger};
use structopt::StructOpt;
use tokio::net::TcpStream;

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

    /// Preferences that the runner should use.
    ///
    /// Preferences should be of the form `pref.name:value` where value is a
    /// string, boolean, or number.
    #[structopt(long = "pref", number_of_values(1), parse(try_from_str = parse_pref))]
    prefs: Vec<(String, PrefValue)>,

    /// Do not require the runner to become idle before running Firefox.
    #[structopt(long)]
    skip_idle: bool,
}

#[tokio::main]
async fn main() {
    let options = Options::from_args();
    let log = build_logger();
    info!(log, "read command-line options"; "options" => ?options);

    if let Err(e) = fxrecorder(log.clone(), options).await {
        error!(log, "unexpected error"; "error" => %e);
        drop(log);
        exit(1);
    }
}

async fn fxrecorder(log: Logger, options: Options) -> Result<(), Box<dyn Error>> {
    let config: Config = read_config(&options.config_path, "fxrecorder")?;

    if let Some(ref profile_path) = &options.profile_path {
        let meta = tokio::fs::metadata(profile_path).await?;

        if !meta.is_file() {
            return Err(ErrorMessage("profile is not a file").into());
        }
    }

    let session_id = {
        let stream = TcpStream::connect(&config.host).await?;
        info!(log, "Connected"; "peer" => config.host);

        // TODO: Ideally we would split new_session and resume_session into
        //       static methods so that we do not need to specify the recorder here.
        let mut proto = RecorderProto::new(
            log.clone(),
            stream,
            FfmpegRecorder::new(log.clone(), &config.recording),
        );

        proto
            .new_session(
                &options.task_id,
                options.profile_path.as_deref(),
                options.prefs,
            )
            .await?
    };

    info!(log, "Disconnected from runner. Waiting to reconnect...");

    {
        let reconnect = || {
            info!(log, "Attempting re-connection to runner...");
            TcpStream::connect(&config.host)
        };

        // This will attempt to reconnect for 0:30 + 1:00 + 2:00 + 4:00 = 7:30.
        let stream = delayed_exponential_retry(reconnect, Duration::from_secs(30), 4)
            .await
            .map_err(|e| {
                error!(
                    log,
                    "Could not connect to runner";
                    "last_error" => %e.source().unwrap()
                );
                e
            })?;

        info!(log, "Re-connected"; "peer" => config.host);

        let mut proto = RecorderProto::new(
            log.clone(),
            stream,
            FfmpegRecorder::new(log.clone(), &config.recording),
        );

        let idle = if options.skip_idle {
            Idle::Skip
        } else {
            Idle::Wait
        };
        proto.resume_session(&session_id, idle).await?;
    }

    Ok(())
}
