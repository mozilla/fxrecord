// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::borrow::Cow;
use std::ffi::OsStr;
use std::fs::{create_dir_all, read_dir, File};
use std::io::{self, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use image::{GenericImageView, ImageError, Rgb};
use itertools::Itertools;
use libfxrecord::ORANGE;
use serde::{Deserialize, Serialize};
use slog::{error, info, warn};
use thiserror::Error;

use crate::ffmpeg::{run_ffmpeg, FfmpegError};

#[derive(Debug, Error)]
#[error("Could not crop video: {}", .0)]
pub struct CropVideoError(#[source] pub FfmpegError);

/// Crop the video.
pub fn crop_video(
    log: slog::Logger,
    video_path: &Path,
    target_directory: &Path,
) -> Result<PathBuf, CropVideoError> {
    // The task bar is 40px tall, but we include an extra px of height to
    // account for blurring from compression.
    const TASK_BAR_CROP: &str = "crop=in_w:in_h-41:0:0";

    let output_path = target_directory.join("cropped.mp4");
    let args = vec![
        OsStr::new("-i"),
        video_path.as_os_str(),
        OsStr::new("-vf"),
        OsStr::new(TASK_BAR_CROP),
        output_path.as_os_str(),
    ];
    info!(log, "cropping video");

    run_ffmpeg(log.clone(), &args).map_err(CropVideoError)?;

    Ok(output_path)
}

#[derive(Debug, Error)]
pub enum ExtractFramesError {
    #[error("Could not create frame directory `{}': {}", .1.display(), .0)]
    CreateDir(#[source] io::Error, PathBuf),

    #[error(transparent)]
    Ffmpeg(FfmpegError),
}

/// Extract the individual frames from the video. The frames are output to
/// `directory` in the form of `directory/frames/NNNNNN.png`, where N is a six
/// digit timestamp of each frame.
///
/// Not all frames are extracted. We use video filters to only extract
/// sequentially different frames.
pub fn extract_frames(
    log: slog::Logger,
    video_path: &Path,
    target_directory: &Path,
) -> Result<PathBuf, ExtractFramesError> {
    let frames_dir = target_directory.join("frames");

    create_dir_all(&frames_dir)
        .map_err(|source| ExtractFramesError::CreateDir(source, frames_dir.clone()))?;

    let output_format = frames_dir.join("%06d.png");

    let args = vec![
        OsStr::new("-i"),
        video_path.as_os_str(),
        // Pass through frames with their timestmaps from the demuxer to the muxer.
        OsStr::new("-vsync"),
        OsStr::new("passthrough"),
        // Use the image2 format.
        OsStr::new("-f"),
        OsStr::new("image2"),
        // Use the "presentation time stamp" (frame number from 0) in the output. The
        // timestamp of each frame can be found by multiplying the frame number by
        // the "time base" (1 / framerate).
        OsStr::new("-frame_pts"),
        OsStr::new("true"),
        // mpdecimate drops sequentially similar frames from the output. This
        // reduces the number of rendered frames from a few thousand to around a
        // hundred.
        OsStr::new("-vf"),
        OsStr::new("mpdecimate"),
        // The output file path format.
        output_format.as_os_str(),
    ];

    info!(log, "extracting frames"; "args" => ?&args);

    run_ffmpeg(log.clone(), &args).map_err(ExtractFramesError::Ffmpeg)?;
    Ok(frames_dir)
}

/// Information about a frame being processed in
/// [`find_first_orange_frame`][function.find_first_orange_frame.html].
#[derive(Debug)]
struct FrameInfo {
    /// The path to the frame.
    path: PathBuf,

    /// The frame number.
    frame_num: u32,
}

/// Squared Euclidean Distance between two colours as 3-vectors.
fn squared_distance(a: &Rgb<u8>, b: &Rgb<u8>) -> i64 {
    let dr = a[0] as i64 - b[0] as i64;
    let dg = a[1] as i64 - b[1] as i64;
    let db = a[2] as i64 - b[2] as i64;

    dr * dr + dg * dg + db * db
}

#[derive(Debug, Error)]
pub enum OrangeError {
    #[error("could not read frame directory: {}", .0)]
    ReadDir(#[source] io::Error),

    #[error("could not read file `{}': {}", .1.display(), .0)]
    Open(#[source] io::Error, PathBuf),

    #[error("could not load image `{}': {}'", .1.display(), .0)]
    Load(#[source] ImageError, PathBuf),

    #[error("no orange frame detected")]
    MissingOrange,
}

/// Return the frame number of the first orange frame of the video.
fn find_first_orange_frame(log: slog::Logger, frames_dir: &Path) -> Result<u32, OrangeError> {
    // The x and y dimensions of the region to sample.
    const SAMPLE_SIZE: u32 = 50;

    // The maximum squared Euclidean distance we will accept between a colour and ORANGE.
    //
    // Non-orange frames are in the range of 10 000.
    const THRESHOLD: i64 = 500;

    // This is the orange that Splash generates and that visuametrics.py expects.
    let orange = image::Rgb(ORANGE);

    let mut frames = vec![];
    for entry in read_dir(frames_dir).map_err(OrangeError::ReadDir)? {
        let entry = entry.map_err(OrangeError::ReadDir)?;
        let path = entry.path();
        let path_str = String::from(path.file_name().unwrap().to_str().unwrap());

        let suffix_pos = match path_str.find(".png") {
            Some(pos) => pos,
            None => {
                warn!(
                    log,
                    "unexpected non-PNG file found in frames directory; skipping";
                    "filename" => path.display()
                );
                continue;
            }
        };

        let frame_num: u32 = match path_str[..suffix_pos].parse() {
            Ok(n) => n,
            Err(_) => {
                warn!(
                    log,
                    "unexpected PNG file with non-numeric filename; skipping";
                    "filename" => path.display(),
                );
                continue;
            }
        };

        frames.push(FrameInfo { path, frame_num });
    }

    frames.sort_by(|a, b| a.frame_num.cmp(&b.frame_num));

    for info in &frames {
        let f = BufReader::new(
            File::open(&info.path)
                .map_err(|source| OrangeError::Open(source, info.path.clone()))?,
        );
        let image = image::load(f, image::ImageFormat::Png)
            .map_err(|source| OrangeError::Load(source, info.path.clone()))?
            .into_rgb();

        let x = (image.width() - SAMPLE_SIZE) / 2;
        let y = (image.height() - SAMPLE_SIZE) / 2;

        let avg = average_image(&image.view(x, y, SAMPLE_SIZE, SAMPLE_SIZE));
        if squared_distance(&avg, &orange) < THRESHOLD {
            return Ok(info.frame_num);
        }
    }

    Err(OrangeError::MissingOrange)
}

/// Compute the average colour of an image.
fn average_image<I>(image: &I) -> Rgb<u8>
where
    I: GenericImageView<Pixel = Rgb<u8>>,
{
    let area = image.width() as u64 * image.height() as u64;

    let mut sum = [0u64; 3];
    for (_, _, pixel) in image.pixels() {
        sum[0] += pixel[0] as u64;
        sum[1] += pixel[1] as u64;
        sum[2] += pixel[2] as u64;
    }

    Rgb([
        (sum[0] / area) as u8,
        (sum[1] / area) as u8,
        (sum[2] / area) as u8,
    ])
}

#[derive(Debug, Deserialize, Serialize)]
pub struct VisualMetrics {
    #[serde(rename = "videoRecordingStart")]
    video_recording_start: u32,

    #[serde(rename = "FirstVisualChange")]
    first_visual_change: u32,

    #[serde(rename = "LastVisualChange")]
    last_visual_change: u32,

    #[serde(rename = "SpeedIndex")]
    speed_index: u32,

    #[serde(rename = "VisualProgress")]
    visual_progress: String,
}

#[derive(Debug, Error)]
pub enum VisualMetricsError {
    #[error("Error executing visualmetrics.py: {}", .0)]
    Exec(#[source] std::io::Error),

    #[error("Could not wait for visualmetrics.py to exit: {}", .0)]
    Wait(#[source] std::io::Error),

    #[error("visualmetrics.py exited with non-zero status code: {}", .0)]
    ExitCode(i32),

    #[error(transparent)]
    Orange(#[from] OrangeError),

    #[error("Could not parse output of visualmetrics.py as JSON: {}", .0)]
    Parse(#[from] serde_json::Error),

    #[error("Could not parse visual progress: {}", .0)]
    VisualProgress(#[from] VisualProgressError),

    #[error("Could not extract frames from video: {}", .0)]
    ExtractFrames(#[from] ExtractFramesError),
}

/// Compute visual metrics with visualmetrics.py
pub fn compute_visual_metrics(
    log: slog::Logger,
    vismet_path: &Path,
    video: &Path,
    target_directory: &Path,
) -> Result<VisualMetrics, VisualMetricsError> {
    // The time base is the reciprocal of the frame rate (units of `s`);
    const TIME_BASE: f64 = 1.0 / 60.0;

    info!(log, "running visual metrics...");

    let output = Command::new("python")
        .args(&[
            vismet_path.as_os_str(),
            OsStr::new("-vvv"),
            OsStr::new("--logformat"),
            OsStr::new("%(levelname)s  %(message)s"),
            OsStr::new("--video"),
            video.as_os_str(),
            OsStr::new("--orange"),
            OsStr::new("--json"),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(VisualMetricsError::Exec)?
        .wait_with_output()
        .map_err(VisualMetricsError::Wait)?;

    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        error!(
            log,
            "visualmetrics.py encountered an error";
            "status" => output.status.code().unwrap(),
            "stdout" => %stdout,
            "stderr" => %stderr,
        );

        return Err(VisualMetricsError::ExitCode(output.status.code().unwrap()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    info!(
        log,
        "ran visualmetrics.py";
        "log" => %String::from_utf8_lossy(&output.stderr),
        "output" => %stdout,
    );

    let metrics: VisualMetrics = serde_json::from_str(&stdout)?;
    let frames_dir = extract_frames(log.clone(), video, target_directory)?;
    let orange_frame_num = find_first_orange_frame(log.clone(), &frames_dir)?;

    // We paint an orange frame *after* we have start firefox, so we want to
    // find the timestamp directly before this frame was painted.
    let start_timestamp = ((orange_frame_num as f64) * TIME_BASE * 1000.0) as u32;
    metrics.normalize(start_timestamp).map_err(Into::into)
}

#[derive(Clone, Debug, Error)]
pub enum VisualProgressError {
    #[error("VisualProgress did not contain =")]
    MissingEquals,

    #[error("Could not parse timestamp in VisualProgress: {}", .0)]
    ParseKey(#[from] std::num::ParseIntError),
}

impl VisualMetrics {
    /// Normalize the metrics so that the video start is at zero.
    pub fn normalize(
        &self,
        orange_frame_timestamp: u32,
    ) -> Result<VisualMetrics, VisualProgressError> {
        assert!(
            self.video_recording_start != 0,
            "VisualMetrics should have orange frames"
        );

        // Consider the following timeline:
        //
        //   time ->
        //   |--|-----------------|-|----------|
        //   ^  ^                 ^ ^          ^
        //   |  |                 | |          \ last_visual_change
        //   |  |                 | \ first_visual_change
        //   |  |                 \ video_recording_start
        //   |   \ orange_frame_timestamp
        //   \ video start (0)
        //
        // visualmetrics.py will ignore all the orange frames at the start.
        // However, we want to include them in the computations since we only
        // paint them once we have started Firefox.
        let orange_duration = self.video_recording_start - orange_frame_timestamp;

        // We normalize metrics so that video_recording_start becomes
        // `orange_frame_timestamp`.
        let video_recording_start = orange_frame_timestamp;

        //  visualmetrics.py detects the first white frame as as the
        //  `video_recording_start`, but that is actually the first frame we have
        //  painted (before that it was orange), so really that is the
        //  `first_visual_change`.
        let first_visual_change = self.video_recording_start;

        // The last_visual_change is just shifted by the duration we di
        let last_visual_change = self.last_visual_change + orange_duration;

        // Speed index is 1 - the integral of visual progress. Our normalization
        // just changes the t = 0 point of the graph, so we can offset it by 100
        // * video_recording_start;
        let speed_index = self.speed_index + 100 * orange_duration;

        // Each entry of the visual progress (except 0) needs to be adjusted to
        // include the orange duration.
        let visual_progress = self
            .visual_progress
            .split(", ")
            .enumerate()
            .map(|(i, kvp)| -> Result<Cow<str>, VisualProgressError> {
                if i == 0 {
                    Ok(Cow::Borrowed(kvp))
                } else {
                    let idx = kvp.find('=').ok_or(VisualProgressError::MissingEquals)?;

                    let (key, value) = kvp.split_at(idx);
                    let key: u32 = key.parse()?;
                    // value will include the =.
                    Ok(Cow::Owned(format!("{}{}", key + orange_duration, value)))
                }
            })
            .intersperse(Ok(Cow::Borrowed(", ")))
            .collect::<Result<String, _>>()?;

        Ok(VisualMetrics {
            video_recording_start,
            first_visual_change,
            last_visual_change,
            speed_index,
            visual_progress,
        })
    }
}
