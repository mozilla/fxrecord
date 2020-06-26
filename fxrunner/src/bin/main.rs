// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::time::Duration;

use libfxrecord::{run, CommonOptions};
use libfxrunner::config::Config;
use libfxrunner::osapi::{WindowsPerfProvider, WindowsShutdownProvider};
use libfxrunner::proto::RunnerProto;
use libfxrunner::taskcluster::FirefoxCi;
use slog::{info, Logger};
use structopt::StructOpt;
use tokio::net::TcpListener;
use tokio::time::delay_for;

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
    /// Whether or not we should skip the actual restart.
    ///
    /// Can only ever be true in a debug build.
    #[cfg(debug_assertions)]
    fn skip_restart(&self) -> bool {
        self.skip_restart
    }

    /// Whether or not we should skip the actual restart.
    ///
    /// Will always be false.
    #[cfg(not(debug_assertions))]
    fn skip_restart(&self) -> bool {
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
            info!(log, "Waiting for connection...");

            let (stream, addr) = listener.accept().await?;
            info!(log, "Received connection"; "peer" => addr);

            if RunnerProto::handle_request(
                log.clone(),
                stream,
                shutdown_provider(&options),
                FirefoxCi::default(),
                WindowsPerfProvider::default(),
            )
            .await?
            {
                break;
            }

            info!(log, "Client disconnected");
        }

        info!(log, "Client disconnected for restart");
        drop(listener);

        if options.skip_restart() {
            // We are skipping doing an actual restart here. We disconnect
            // our socket and the listener and wait 30 seconds. This is
            // enough time for the socket to get recycled by the operating
            // system so that we don't run into address reuse issues.
            //
            // It also allows us to (manually) test the exponential backoff
            // re-connection in the recorder.

            info!(log, "\"Restarting\" ... ");
            delay_for(Duration::from_secs(30)).await;
            info!(log, "\"Restarted\"");
        } else {
            break Ok(());
        }
    }
}

#[cfg(debug_assertions)]
fn shutdown_provider(options: &Options) -> WindowsShutdownProvider {
    WindowsShutdownProvider::skipping_restart(options.skip_restart)
}

#[cfg(not(debug_assertions))]
fn shutdown_provider(_: &Options) -> WindowsShutdownProvider {
    WindowsShutdownProvider::default()
}
