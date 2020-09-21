// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::fs::{create_dir_all, File};
use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;
use zip::ZipArchive;

/// Statistics about an unzip operation.
#[derive(Default)]
pub struct ZipStats {
    /// The number of extracted files.
    pub extracted: usize,

    /// The top-level directory of the zip file.
    pub top_level_dir: Option<PathBuf>,
}

/// Unzip the archive at the given location to the target location.
pub fn unzip(archive: &Path, target: &Path) -> Result<ZipStats, ZipError> {
    let mut stats = ZipStats::default();

    let zip_file = File::open(archive).map_err(|source| ZipError::OpenArchive {
        archive: archive.into(),
        source,
    })?;

    let mut zip = ZipArchive::new(zip_file).map_err(|source| ZipError::ReadArchive {
        archive: archive.into(),
        source,
    })?;

    for i in 0..zip.len() {
        let mut zipped = zip.by_index(i).map_err(|source| ZipError::ReadArchive {
            archive: archive.into(),
            source,
        })?;

        let name = zipped.sanitized_name();
        let path = target.join(&name);

        if i == 0 {
            stats.top_level_dir = Some(name.clone());
        } else if let Some(top_level_dir) = stats.top_level_dir.take() {
            stats.top_level_dir = common_stem(&top_level_dir, &name);
        }

        if zipped.is_dir() {
            create_dir_all(&path).map_err(|source| ZipError::MakeDir { path, source })?;
            continue;
        }

        debug_assert!(zipped.is_file());

        let parent = path.parent().expect("path has no parent directory");
        create_dir_all(&parent).map_err(|source| ZipError::MakeDir {
            path: parent.into(),
            source,
        })?;

        let mut writer = File::create(&path).map_err(|source| ZipError::Io {
            archive: archive.into(),
            file_name: path.clone(),
            source,
        })?;

        io::copy(&mut zipped, &mut writer).map_err(|source| ZipError::Io {
            archive: archive.into(),
            file_name: path,
            source,
        })?;

        stats.extracted += 1;
    }

    Ok(stats)
}

fn common_stem(p1: &Path, p2: &Path) -> Option<PathBuf> {
    let mut common = None;

    use std::path::Component;

    for (c1, c2) in Iterator::zip(p1.components(), p2.components()) {
        if let Component::Normal(c1) = c1 {
            if let Component::Normal(c2) = c2 {
                if c1 == c2 {
                    common.get_or_insert_with(PathBuf::new).push(c1);
                    continue;
                }
            }
        }

        break;
    }

    common
}

#[derive(Debug, Error)]
pub enum ZipError {
    #[error(
        "Could not open zip archive `{}': {}",
        .archive.display(),
        .source
    )]
    OpenArchive { archive: PathBuf, source: io::Error },

    #[error(
        "could not read zip archive `{}': {}",
        .archive.display(),
        .source
    )]
    ReadArchive {
        archive: PathBuf,
        source: zip::result::ZipError,
    },

    #[error(
        "IO error while extracting file `{}' from archive `{}': {}",
        .file_name.display(),
        .archive.display(),
        source
    )]
    Io {
        archive: PathBuf,
        file_name: PathBuf,
        source: io::Error,
    },

    #[error(
        "could not make required directory `{}': {}",
        .path.display(),
        .source
    )]
    MakeDir { path: PathBuf, source: io::Error },
}

#[cfg(test)]
mod test {
    use std::env::current_dir;
    use std::path::{Path, PathBuf};

    use tempfile::TempDir;

    use super::{common_stem, unzip};

    #[test]
    fn test_zip() {
        let test_dir = current_dir().unwrap().parent().unwrap().join("test");
        {
            let zip = test_dir.join("test.zip");
            let tempdir = TempDir::new().unwrap();

            let stats = unzip(&zip, tempdir.path()).unwrap();

            let dir_path = tempdir.path().join("dir");
            assert!(dir_path.is_dir());
            assert!(dir_path.join("test.txt").is_file());
            assert!(tempdir.path().join("empty").is_dir());

            assert_eq!(stats.extracted, 1);
            assert_eq!(stats.top_level_dir, None);
        }

        {
            let zip = test_dir.join("profile.zip");
            let tempdir = TempDir::new().unwrap();

            let stats = unzip(&zip, tempdir.path()).unwrap();

            assert!(tempdir.path().join("places.sqlite").is_file());
            assert!(tempdir.path().join("prefs.js").is_file());
            assert!(tempdir.path().join("user.js").is_file());

            assert_eq!(stats.extracted, 3);
            assert_eq!(stats.top_level_dir, None);
        }

        {
            let zip = test_dir.join("profile_nested.zip");
            let tempdir = TempDir::new().unwrap();

            let stats = unzip(&zip, tempdir.path()).unwrap();
            let profile_dir = tempdir.path().join("profile");

            assert!(profile_dir.is_dir());
            assert!(profile_dir.join("places.sqlite").is_file());
            assert!(profile_dir.join("prefs.js").is_file());
            assert!(profile_dir.join("user.js").is_file());

            assert_eq!(stats.extracted, 3);
            assert_eq!(stats.top_level_dir, Some(PathBuf::from("profile")));
        }
    }

    #[test]
    fn test_common_stem() {
        assert_eq!(
            common_stem(Path::new("foo/bar/baz"), Path::new("foo/bar/baz")),
            Some(PathBuf::from("foo/bar/baz")),
        );

        assert_eq!(
            common_stem(Path::new("foo/bar/baz"), Path::new("foo/bar/qux")),
            Some(PathBuf::from("foo/bar"))
        );

        assert_eq!(
            common_stem(Path::new("foo/bar/baz"), Path::new("foo/baz/bar")),
            Some(PathBuf::from("foo"))
        );

        // Path normalization.
        assert_eq!(
            common_stem(Path::new("foo/"), Path::new("foo")),
            Some(PathBuf::from("foo"))
        );

        assert_eq!(
            common_stem(Path::new("foo/bar"), Path::new("baz/qux")),
            None,
        );
    }
}
