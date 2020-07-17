// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::error::Error;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use libfxrecord::{run, CommonOptions};
use libfxrunner::config::Config;
use libfxrunner::osapi::{WindowsPerfProvider, WindowsShutdownProvider};
use libfxrunner::proto::RunnerProto;
use libfxrunner::request::FsRequestManager;
use libfxrunner::taskcluster::FirefoxCi;
use slog::{error, info, warn, Logger};
use structopt::StructOpt;
use tokio::fs::create_dir_all;
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
    if let Err(e) = create_dir_all(&config.requests_dir).await {
        error!(
            log,
            "Could not create requests directory";
            "requests_dir" => config.requests_dir.display(),
            "error" => %e,
        );

        return Err(e.into());
    }

    loop {
        let mut listener = TcpListener::bind(&config.host).await?;

        loop {
            info!(log, "Waiting for connection...");

            let (stream, addr) = listener.accept().await?;
            info!(log, "Received connection"; "peer" => addr);

            let result = RunnerProto::handle_request(
                log.clone(),
                stream,
                shutdown_provider(&options),
                FirefoxCi::default(),
                WindowsPerfProvider::default(),
                FsRequestManager::new(log.clone(), &config.requests_dir),
            )
            .await;

            match result {
                Ok(restart) => {
                    if restart {
                        break;
                    }
                }
                Err(e) => {
                    error!(log, "Encountered an unexpected error while serving a request"; "error" => %e);
                }
            }

            info!(log, "Client disconnected");

            // We aren't restarting, which means we handled a resume request. We
            // only expect a single pending request at a time, so the request
            // directory *should* be empty. If it isn't, then isn't empty it.
            if let Err(e) = cleanup_requests_dir(log.clone(), &config.requests_dir).await {
                error!(log, "Could not cleanup requests directory"; "error" => %e);
            }
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

async fn cleanup_requests_dir(log: slog::Logger, path: &Path) -> Result<(), io::Error> {
    let mut entries = tokio::fs::read_dir(path).await?;

    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        warn!(log, "requests directory was not empty"; "path" => path.display());
        tokio::fs::remove_dir_all(&path).await?;
    }

    Ok(())
}
