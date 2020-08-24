// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::net::SocketAddr;
use std::path::PathBuf;

use serde::Deserialize;

/// The configuration for FxRecorder.
#[derive(Debug, Deserialize)]
pub struct Config {
    /// The address of the `fxrunner` to connect to.
    pub host: SocketAddr,

    /// The recording configuraton.
    pub recording: RecordingConfig,
}

/// Recording-specific configuration.
#[derive(Clone, Debug, Deserialize)]
pub struct RecordingConfig {
    /// The path to the `ffmpeg` executable.`
    pub ffmpeg_path: PathBuf,

    /// The name of the video capture device.
    ///
    /// This can be found via running:
    /// ```text
    /// ffmpeg -f dshow -list_devices true -i dummy
    /// ```
    ///
    /// This will be used to generate the `-i` argument to `ffmpeg`.
    pub device: String,

    /// The size of the video stream.
    ///
    /// This corresponds to the `-video_size` argument to `ffmpeg`.
    pub video_size: Size,

    /// The frame rate to capture.
    ///
    /// This corresponds to the `-framerate` argument to `ffmpeg`.
    pub frame_rate: u8,

    /// The output size of the video.
    ///
    /// If provided, the video will be scaled to this size. Otherwise, the video
    /// will be the size recorded in
    /// [`video_size`](struct.CaptureConfig.html#structfield.video_size).CaptureConfig
    ///
    /// This is used to generate the `-vf` argument to `ffmpeg`.
    pub output_size: Option<Size>,

    /// The buffer size to use when recording.
    ///
    /// This corresponds to the `-rtbufsize` argument to `ffmpeg`.
    pub buffer_size: String,

    /// The minimum recording time. `ffmpeg` will record for at least this long.
    pub minimum_recording_time_secs: u8,
}

/// The size of a video.
#[derive(Copy, Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct Size {
    /// The size in the y dimension.
    pub y: u16,
    /// The size in the x dimension.
    pub x: u16,
}
