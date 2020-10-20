// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::ffi::OsStr;
use std::io;
use std::process::{Command, Stdio};

use slog::{error, info};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum FfmpegError {
    #[error("Could not start ffmpeg: {}", .0)]
    Spawn(#[source] io::Error),

    #[error("Error waiting for ffmpeg to exit: {}", .0)]
    Wait(#[source] io::Error),

    #[error("ffmpeg exited with non-zero status: {}", .0)]
    ExitCode(i32),
}

pub fn run_ffmpeg(log: slog::Logger, args: &[&OsStr]) -> Result<(), FfmpegError> {
    info!(log, "executing ffmpeg"; "args" => ?args);

    let output = Command::new("ffmpeg")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(FfmpegError::Spawn)?
        .wait_with_output()
        .map_err(FfmpegError::Wait)?;

    if output.status.success() {
        Ok(())
    } else {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let status = output.status.code().unwrap();

        error!(
            log,
            "ffmpeg exited with non-zero status";
            "status" => status,
            "stdout" => %stdout,
            "stderr" => %stderr,
        );

        Err(FfmpegError::ExitCode(status))
    }
}
