// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use thiserror::Error;
use toml::{self, Value};

/// Read the given section from the given configuration file and deserialize it as a `T`.
pub fn read_config<T, P>(path: P, section: &'static str) -> Result<T, ConfigError>
where
    for<'de> T: Deserialize<'de>,
    P: AsRef<Path>,
{
    let path = path.as_ref();
    let mut f = File::open(path).map_err(|e| ConfigError::OpenConfig {
        path: path.into(),
        source: e,
    })?;
    let mut buf = String::new();

    f.read_to_string(&mut buf)
        .map_err(|e| ConfigError::ReadConfig {
            path: path.into(),
            source: e,
        })?;

    toml::from_str::<Value>(&buf)
        .map_err(|e| ConfigError::Parse {
            path: path.into(),
            source: e,
        })
        .and_then(|mut value| {
            value
                .as_table_mut()
                .and_then(|table| table.remove(section))
                .ok_or_else(|| ConfigError::MissingSection {
                    path: PathBuf::from(path),
                    section,
                })
        })
        .and_then(|val| {
            val.try_into().map_err(|e| ConfigError::Parse {
                path: path.into(),
                source: e,
            })
        })
}

/// An error occurred while loading or parsing a configuration file.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// The file could not be opened.
    #[error("Could not open config file `{}': {}", .path.display(), .source)]
    OpenConfig { path: PathBuf, source: io::Error },

    /// The file could not be read.
    #[error("Could not read config file `{}': {}", .path.display(), source)]
    ReadConfig { path: PathBuf, source: io::Error },

    /// The required section was missing from the config file.
    #[error("Missing `{}' section in config file `{}'", .section, .path.display())]
    MissingSection {
        path: PathBuf,
        section: &'static str,
    },

    /// The file could not be parsed.
    #[error("Could not parse config file `{}': {}", .path.display(), .source)]
    Parse {
        path: PathBuf,
        source: toml::de::Error,
    },
}
