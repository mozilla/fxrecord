// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use libfxrecord::error::ErrorMessage;
use libfxrecorder::proto::RecorderProto;
use libfxrunner::proto::RunnerProto;
use libfxrunner::shutdown::ShutdownProvider;
use slog::Logger;
use tokio::net::{TcpListener, TcpStream};

#[derive(Default)]
pub struct TestShutdownProvider {
    error: Option<&'static str>,
}

impl TestShutdownProvider {
    pub fn with_error(s: &'static str) -> Self {
        TestShutdownProvider {
            error: Some(s),
        }
    }
}

impl ShutdownProvider for TestShutdownProvider {
    type Error = ErrorMessage<&'static str>;

    fn initiate_restart(&self, _reason: &str) -> Result<(), Self::Error> {
        match self.error {
            Some(ref e) => Err(ErrorMessage(e)),
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
        let should_restart = RunnerProto::new(test_logger(), runner, TestShutdownProvider { error: None })
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
