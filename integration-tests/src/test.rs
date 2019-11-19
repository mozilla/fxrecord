// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use fxrecorder::proto::RecorderProto;
use fxrunner::proto::RunnerProto;
use fxrunner::shutdown::Shutdown;
use libfxrecord::error::ErrorMessage;
use slog::Logger;
use tokio::net::{TcpListener, TcpStream};

#[derive(Default)]
pub struct TestShutdown {
    error: Option<String>,
}

impl Shutdown for TestShutdown {
    type Error = ErrorMessage<String>;

    fn initiate_restart(&self, _reason: &str) -> Result<(), Self::Error> {
        match self.error {
            Some(ref e) => Err(ErrorMessage(e.into())),
            None => Ok(()),
        }
    }
}

fn test_logger() -> Logger {
    Logger::root(slog::Discard, slog::o! {})
}

#[tokio::test]
async fn test_handshake() {
    let mut listener = TcpListener::bind("127.0.0.1:9999").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        let (runner, _) = listener.accept().await.unwrap();
        let should_restart = RunnerProto::new(test_logger(), runner, TestShutdown { error: None })
            .handshake_reply()
            .await
            .unwrap();

        assert!(should_restart);
    });

    let recorder = TcpStream::connect(&addr).await.unwrap();
    RecorderProto::new(test_logger(), recorder)
        .handshake(true)
        .await
        .unwrap();
}
