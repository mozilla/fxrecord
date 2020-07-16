// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::borrow::Cow;
use std::io;
use std::iter;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use rand::distributions::Alphanumeric;
use rand::prelude::*;
use thiserror::Error;
use tokio::fs::create_dir;

use crate::fs::PathExt;

const REQUEST_ID_LEN: usize = 32;

#[derive(Clone)]
pub struct RequestInfo<'a> {
    pub id: Cow<'a, str>,
    pub path: PathBuf,
}
/// A trait for creating and validating requests.
#[async_trait]
pub trait RequestManager {
    /// Create a new request.
    async fn new_request(&self) -> Result<RequestInfo<'static>, NewRequestError>;

    /// Attempt to resume a request with the given ID.
    ///
    /// If the request ID is valid and a prior request exists, the path to its
    /// directory will be returned.
    async fn resume_request<'a>(
        &self,
        request_id: &'a str,
    ) -> Result<RequestInfo<'a>, ResumeRequestError>;

    /// Ensure the profile directory for the given request exists and is valid
    /// (i.e., contains everything to do a recordering).
    async fn ensure_valid_profile_dir<'a>(
        &self,
        request_info: &RequestInfo<'a>,
    ) -> Result<PathBuf, io::Error>;
}

pub struct FsRequestManager {
    path: PathBuf,
}

impl FsRequestManager {
    pub fn new(path: &Path) -> Self {
        FsRequestManager { path: path.into() }
    }
}

#[async_trait]
impl RequestManager for FsRequestManager {
    async fn new_request(&self) -> Result<RequestInfo<'static>, NewRequestError> {
        const TRIES: u64 = 30;

        let mut request_id = String::with_capacity(REQUEST_ID_LEN);
        let mut path = self.path.clone();

        // Generate a request id for the runner. It will be used as the name of
        // a directory to store the downloaded Firefox binary and profile. If a
        // directory already exists with the generated name (which is very
        // unlikely), try again until either generate an unused name or run out
        // of attempts.
        for _ in 0..TRIES {
            {
                let mut rng = thread_rng();
                request_id.extend(
                    iter::repeat(())
                        .map(|_| rng.sample(Alphanumeric))
                        .take(REQUEST_ID_LEN),
                );
            }

            path.push(&request_id);

            if let Err(e) = create_dir(&path).await {
                if e.kind() == io::ErrorKind::AlreadyExists {
                    request_id.clear();
                    path.pop();
                    continue;
                } else {
                    return Err(e.into());
                }
            }

            return Ok(RequestInfo {
                path,
                id: Cow::Owned(request_id),
            });
        }

        Err(NewRequestError::TooManyAttempts(TRIES))
    }

    async fn resume_request<'a>(
        &self,
        request_id: &'a str,
    ) -> Result<RequestInfo<'a>, ResumeRequestError> {
        if !validate_request_id(request_id) {
            return Err(ResumeRequestError {
                kind: ResumeRequestErrorKind::InvalidId,
                request_id: request_id.into(),
            });
        }

        let path = self.path.join(request_id);

        if !path.is_dir_async().await {
            return Err(ResumeRequestError {
                kind: ResumeRequestErrorKind::DoesNotExist,
                request_id: request_id.into(),
            });
        }

        if !path.join("profile").is_dir_async().await {
            return Err(ResumeRequestError {
                kind: ResumeRequestErrorKind::MissingProfile,
                request_id: request_id.into(),
            });
        }

        let firefox_path = path.join("firefox");
        let bin_path = firefox_path.join("firefox.exe");
        if !firefox_path.is_dir_async().await || !bin_path.is_file_async().await {
            return Err(ResumeRequestError {
                kind: ResumeRequestErrorKind::MissingFirefox,
                request_id: request_id.into(),
            });
        }

        Ok(RequestInfo {
            path,
            id: Cow::Borrowed(request_id),
        })
    }

    async fn ensure_valid_profile_dir<'a>(
        &self,
        request_info: &RequestInfo<'a>,
    ) -> Result<PathBuf, io::Error> {
        let profile_path = request_info.path.join("profile");
        create_dir(&profile_path).await?;
        Ok(profile_path)
    }
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ResumeRequestErrorKind {
    #[error("ID contains invalid characters")]
    InvalidId,

    #[error("does not exist to resume")]
    DoesNotExist,

    #[error("missing a profile directory")]
    MissingProfile,

    #[error("missing a Firefox binary")]
    MissingFirefox,
}

#[derive(Debug, Eq, Error, PartialEq)]
#[error("Invalid request `{}': {}", .request_id, .kind)]
pub struct ResumeRequestError {
    pub request_id: String,
    pub kind: ResumeRequestErrorKind,
}

#[derive(Debug, Error)]
pub enum NewRequestError {
    #[error(transparent)]
    Io(#[from] io::Error),

    #[error("Could not create a request directory after {} attempts", .0)]
    TooManyAttempts(u64),
}

/// Validate the given request ID is of the proper form.
fn validate_request_id(request_id: &str) -> bool {
    request_id.len() == REQUEST_ID_LEN && request_id.chars().all(|c| c.is_ascii_alphanumeric())
}
