use fxrecorder::proto::RecorderProto;
use fxrunner::proto::RunnerProto;
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
        RunnerProto::new(test_logger(), runner)
            .handshake_reply()
            .await
            .unwrap();
    });

    let recorder = TcpStream::connect(&addr).await.unwrap();
    RecorderProto::new(test_logger(), recorder)
        .handshake()
        .await
        .unwrap();
}
