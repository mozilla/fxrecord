// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

mod config;
mod service;

use std::error::Error;
use std::path::{Path, PathBuf};

use libfxrecord::service::FxRunnerService;
use libfxrecord::{run, CommonOptions};
use slog::{info, Logger};
use structopt::StructOpt;
use tarpc::server::{BaseChannel, Channel};
use tokio::stream::StreamExt;
use tokio_serde::formats::Bincode;

use crate::config::Config;

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
    let mut transport = tarpc::serde_transport::tcp::listen(&config.host, Bincode::default).await?;
    loop {
        let connection = transport.next().await.unwrap().unwrap();
        let addr = connection.peer_addr().unwrap();

        info!(log, "Received connection"; "peer" => %addr);

        let server = service::FxRunner {};
        let channel = BaseChannel::with_defaults(connection);

        let handler = channel.respond_with(server.serve());

        handler.execute().await;
    }
}
