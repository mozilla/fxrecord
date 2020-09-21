// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::error::Error;
use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Output, Stdio};
use std::time::Duration;

use async_trait::async_trait;
use slog::{error, info};
use thiserror::Error;
use tokio::prelude::*;
use tokio::process::{ChildStdin, Command};
use tokio::task::JoinError;
use tokio::time::delay_for;

use crate::config::RecordingConfig;

/// A trait representing the ability to do video recording.
#[async_trait]
pub trait Recorder {
    /// A handle to the recording instance.
    type Handle;

    /// The error type associated with starting or finishping the recording.
    type Error: Error + 'static;

    /// Start a recording in the given directory.
    ///
    /// The returned handle can be passed to
    /// [`finish_recording`](#method.stop_recording) to stop its recording.
    async fn start_recording(&self, directory: &Path) -> Result<Self::Handle, Self::Error>;

    /// Wait for the recording inidicated by `handle` to finish.
    ///
    /// The path to the recording is returned.
    async fn wait_for_recording_finished(
        &self,
        handle: Self::Handle,
    ) -> Result<PathBuf, Self::Error>;
}

/// A Recorder that uses `ffmpeg`.
pub struct FfmpegRecorder<'a> {
    log: slog::Logger,
    config: &'a RecordingConfig,
}

/// A handle for the [`FfmpegRecorder`][FfmpegRecorder]
///
/// [FfmpegRecorder]: struct.FfmpegRecorder.html
pub struct FfmpegRecordingHandle {
    task_join_handle: tokio::task::JoinHandle<Result<Output, io::Error>>,
    output_path: PathBuf,
    ffmpeg_stdin: ChildStdin,
}

impl<'a> FfmpegRecorder<'a> {
    pub fn new(log: slog::Logger, config: &'a RecordingConfig) -> Self {
        FfmpegRecorder { log, config }
    }
}

/// An error recording with ffmpeg.
#[derive(Debug, Error)]
pub enum FfmpegRecordingError {
    #[error("Could not start ffmpeg: {}", .0)]
    Start(#[source] io::Error),

    #[error("Could not ask ffmpeg to quit: {}", .0)]
    WriteQ(#[source] io::Error),

    #[error("could not wait for ffmpeg to exist: {}", .0)]
    Wait(#[source] io::Error),

    #[error("ffmpeg exited with nonzero status: {}", .0)]
    ExitStatus(i32),

    #[error("could not join ffmpeg task: {}", .0)]
    Join(#[from] JoinError),
}

#[async_trait]
impl<'a> Recorder for FfmpegRecorder<'a> {
    type Handle = FfmpegRecordingHandle;
    type Error = FfmpegRecordingError;

    async fn start_recording(&self, recording_dir: &Path) -> Result<Self::Handle, Self::Error> {
        let output_path = recording_dir.join("recording.mp4");
        let input_arg = format!("video={}", self.config.device);
        let video_size_arg = format!("{}x{}", self.config.video_size.x, self.config.video_size.y);
        let framerate_arg = self.config.frame_rate.to_string();

        let mut args: Vec<&OsStr> = vec![
            OsStr::new("-f"),
            OsStr::new("dshow"),
            OsStr::new("-i"),
            OsStr::new(&input_arg),
            OsStr::new("-video_size"),
            OsStr::new(&video_size_arg),
            OsStr::new("-rtbufsize"),
            OsStr::new(&self.config.buffer_size),
            OsStr::new("-framerate"),
            OsStr::new(&framerate_arg),
        ];

        let scale;
        if let Some(ref output_size) = self.config.output_size {
            scale = format!("scale=w={}:h={}", output_size.x, output_size.y);

            args.push(OsStr::new("-vf"));
            args.push(OsStr::new(&scale));
        }

        args.push(output_path.as_os_str());

        info!(
            self.log,
            "starting ffmpeg...";
            "ffmpeg_path" => &self.config.ffmpeg_path.display(),
            "args" => ?&args,
        );
        let mut ffmpeg = Command::new(&self.config.ffmpeg_path)
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(FfmpegRecordingError::Start)?;

        // Child::wait_with_output drops stdin, so we need to acquire it first so we
        // can send a quit message.
        let ffmpeg_stdin = ffmpeg.stdin.take().expect("process has no stdin handle");

        // Launch a separate task that will start buffering output from ffmpeg.
        // If we do nto start buffering, ffmpeg will block on writing output and
        // drop frames.
        let task_join_handle = tokio::spawn(ffmpeg.wait_with_output());

        // Ensure we capture frames *before* the runner paints the start frame.
        delay_for(Duration::from_secs(1)).await;

        Ok(FfmpegRecordingHandle {
            output_path,
            task_join_handle,
            ffmpeg_stdin,
        })
    }

    async fn wait_for_recording_finished(
        &self,
        handle: Self::Handle,
    ) -> Result<PathBuf, Self::Error> {
        let FfmpegRecordingHandle {
            output_path,
            task_join_handle,
            mut ffmpeg_stdin,
        } = handle;

        delay_for(Duration::from_secs(
            self.config.minimum_recording_time_secs as u64,
        ))
        .await;

        info!(self.log, "requesting ffmpeg to finish recording...");

        ffmpeg_stdin
            .write(&b"q"[..])
            .await
            .map_err(FfmpegRecordingError::WriteQ)?;

        let output = task_join_handle
            .await?
            .map_err(FfmpegRecordingError::Wait)?;

        if output.status.success() {
            info!(self.log, "ffmpeg finished recording");
            Ok(output_path)
        } else {
            let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
            let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

            // output.status.code() can only be None on UNIX systems when the
            // process is terminated by a signal.
            let code = output.status.code().unwrap();

            error!(
                self.log,
                "ffmpeg exited unsuccessfully";
                "status" => code,
                "stdout" => stdout,
                "stderr" => stderr,
            );

            Err(FfmpegRecordingError::ExitStatus(code))
        }
    }
}
