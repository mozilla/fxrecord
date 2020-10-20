// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

pub mod config;
pub mod error;
pub mod logging;
pub mod net;
pub mod prefs;

/// The shade of orange visualmetrics.p; expects for pre-recording frames.
pub const ORANGE: [u8; 3] = [222, 100, 13];
