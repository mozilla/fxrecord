// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

/// A test helper that is used to assert an operation either occurred or did not
/// before being dropped.
#[derive(Debug)]
pub struct AssertInvoked {
    name: &'static str,
    invoked: bool,
    should_be_invoked: bool,
}

impl AssertInvoked {
    pub fn new(name: &'static str, should_be_invoked: bool) -> Self {
        AssertInvoked {
            name,
            invoked: false,
            should_be_invoked,
        }
    }

    pub fn invoked(&mut self) {
        assert!(
            self.should_be_invoked,
            "{} was unexpectedly invoked",
            self.name
        );
        self.invoked = true;
    }
}

impl Drop for AssertInvoked {
    fn drop(&mut self) {
        if self.should_be_invoked && !self.invoked {
            panic!("{} dropped without being invoked", self.name);
        }
    }
}

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
        .inspect(|result| {
            if let Ok(ref entry) = result {
                eprintln!("{} contains {:?}", path.display(), entry);
            }
        })
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
        .is_empty()
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

pub async fn touch(path: &Path) -> Result<(), io::Error> {
    tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(path)
        .await
        .map(drop)
}
