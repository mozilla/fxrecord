// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Return the path of the top level `test/` directory.
pub fn test_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("test")
}

pub fn directory_is_empty(path: &Path) -> bool {
    path.read_dir()
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
        .len()
        == 0
}

pub fn assert_populated_profile(profile_dir: &Path) {
    assert!(profile_dir.exists());
    assert!(profile_dir.is_dir());
    assert!(profile_dir.join("places.sqlite").is_file());
    assert!(profile_dir.join("prefs.js").is_file());
    assert!(profile_dir.join("user.js").is_file());
}

pub fn assert_file_contents_eq(path: &Path, expected: &'static str) {
    let contents = {
        let mut buf = String::new();
        let mut f = File::open(path).unwrap();
        f.read_to_string(&mut buf).unwrap();
        buf
    };
    assert_eq!(contents, expected);
}
