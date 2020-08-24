// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::net::SocketAddr;
use std::path::PathBuf;

use serde::Deserialize;

/// The configuration for FxRunner.
#[derive(Debug, Deserialize)]
pub struct Config {
    /// The address and port to listen on.
    pub host: SocketAddr,

    /// The directory to store session state in.
    pub session_dir: PathBuf,

    /// The size of the display.
    pub display_size: Size,
}

/// The size of a video.
#[derive(Copy, Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct Size {
    /// The size in the y dimension.
    pub y: u16,
    /// The size in the x dimension.
    pub x: u16,
}
