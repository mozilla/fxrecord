// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use libfxrecorder::proto::RecorderProto;
use libfxrunner::proto::RunnerProto;
use slog::Logger;
use tokio::net::{TcpListener, TcpStream};

fn test_logger() -> Logger {
    Logger::root(slog::Discard, slog::o! {})
}

#[tokio::test]
async fn test_handshake() {
    let mut listener = TcpListener::bind("127.0.0.1:9999").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        let (runner, _) = listener.accept().await.unwrap();
        let should_restart = RunnerProto::new(test_logger(), runner)
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
