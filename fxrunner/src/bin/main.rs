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

use fxrunner::config::Config;
use fxrunner::proto::RunnerProto;
use fxrunner::shutdown::*;

#[derive(Debug, StructOpt)]
#[structopt(name = "fxrunner", about = "Start FxRunner")]
struct Options {
    /// The configuration file to use.
    #[structopt(long = "config", default_value = "fxrecord.toml")]
    config_path: PathBuf,

    /// Skip the restart when the recorder requests it.
    ///
    /// Only available in debug builds.
    #[cfg(debug_assertions)]
    #[structopt(long)]
    skip_restart: bool,
}

impl Options {
    /// Whether we should skip the actual restart or not.
    ///
    /// Can only ever be true in a debug build.
    #[cfg(debug_assertions)]
    pub fn skip_restart(&self) -> bool {
        self.skip_restart
    }

    /// Whether we should skip the actual restart or not.
    ///
    /// Will always be false.
    #[cfg(not(debug_assertions))]
    pub fn skip_restart(&self) -> bool {
        false
    }
}

impl CommonOptions for Options {
    fn config_path(&self) -> &Path {
        &self.config_path
    }
}

fn main() {
    run::<Options, Config, _, _>(fxrunner, "fxrunner");
}

async fn fxrunner(log: Logger, options: Options, config: Config) -> Result<(), Box<dyn Error>> {
    loop {
        let mut listener = TcpListener::bind(&config.host).await?;

        loop {
            let (stream, addr) = listener.accept().await?;
            info!(log, "Received connection"; "peer" => addr);

            #[cfg(debug_assertions)]
            let mut proto = RunnerProto::new(
                log.clone(),
                stream,
                DebugShutdown {
                    skip_restart: options.skip_restart(),
                },
            );

            #[cfg(not(debug_assertions))]
            let mut proto = RunnerProto::new(log.clone(), stream, WindowsShutdown);

            let restart = proto.handshake_reply().await?;

            if restart {
                if options.skip_restart() {
                    // We are skipping doing an actual restart here. We
                    // disconnect our socket and the listener and wait 30
                    // seconds. This is enough time for the socket to get
                    // recycled by the operating system so that we don't run
                    // into address reuse issues.
                    //
                    // It also allows us to (manually) test the exponential
                    // backoff re-connection in the recorder.

                    drop(proto);
                    drop(listener);

                    info!(log, "\"Restarting\" ... ");
                    delay_for(Duration::from_secs(30)).await;
                    info!(log, "\"Restarted\"");

                    break;
                } else {
                    return Ok(());
                }
            }
        }
    }
}
