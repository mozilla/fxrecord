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
use scopeguard::{guard, ScopeGuard};
use slog::error;
use thiserror::Error;
use tokio::fs::create_dir;

use crate::fs::PathExt;

const REQUEST_ID_LEN: usize = 32;

#[derive(Clone)]
pub struct SessionInfo<'a> {
    pub id: Cow<'a, str>,
    pub path: PathBuf,
}

/// A trait for creating and validating session.
#[async_trait]
pub trait SessionManager {
    /// Create a new session.
    async fn new_session(&self) -> Result<SessionInfo<'static>, NewSessionError>;

    /// Attempt to resume a request with the given ID.
    ///
    /// If the request ID is valid and a prior request exists, the path to its
    /// directory will be returned.
    async fn resume_session<'a>(
        &self,
        session_id: &'a str,
    ) -> Result<SessionInfo<'a>, ResumeSessionError>;

    /// Ensure the profile directory for the given request exists and is valid
    /// (i.e., contains everything to do a recordering).
    async fn ensure_valid_profile_dir<'a>(
        &self,
        session_info: &SessionInfo<'a>,
    ) -> Result<PathBuf, io::Error>;
}

pub struct DefaultSessionManager {
    log: slog::Logger,
    path: PathBuf,
}

impl DefaultSessionManager {
    pub fn new(log: slog::Logger, path: &Path) -> Self {
        DefaultSessionManager {
            log,
            path: path.into(),
        }
    }
}

#[async_trait]
impl SessionManager for DefaultSessionManager {
    async fn new_session(&self) -> Result<SessionInfo<'static>, NewSessionError> {
        const TRIES: u64 = 30;

        let mut session_id = String::with_capacity(REQUEST_ID_LEN);
        let mut path = self.path.clone();

        // Generate a request id for the runner. It will be used as the name of
        // a directory to store the downloaded Firefox binary and profile. If a
        // directory already exists with the generated name (which is very
        // unlikely), try again until either generate an unused name or run out
        // of attempts.
        for _ in 0..TRIES {
            {
                let mut rng = thread_rng();
                session_id.extend(
                    iter::repeat(())
                        .map(|_| rng.sample(Alphanumeric))
                        .take(REQUEST_ID_LEN),
                );
            }

            path.push(&session_id);

            if let Err(e) = create_dir(&path).await {
                if e.kind() == io::ErrorKind::AlreadyExists {
                    session_id.clear();
                    path.pop();
                    continue;
                } else {
                    return Err(e.into());
                }
            }

            return Ok(SessionInfo {
                path,
                id: Cow::Owned(session_id),
            });
        }

        Err(NewSessionError::TooManyAttempts(TRIES))
    }

    async fn resume_session<'a>(
        &self,
        session_id: &'a str,
    ) -> Result<SessionInfo<'a>, ResumeSessionError> {
        if !validate_session_id(session_id) {
            return Err(ResumeSessionError {
                kind: ResumeSessionErrorKind::InvalidId,
                session_id: session_id.into(),
            });
        }

        let path = self.path.join(session_id);

        if !path.is_dir_async().await {
            return Err(ResumeSessionError {
                kind: ResumeSessionErrorKind::DoesNotExist,
                session_id: session_id.into(),
            });
        }

        let session_info = SessionInfo {
            path,
            id: Cow::Borrowed(session_id),
        };

        let cleanup = guard(self.log.clone(), |log| cleanup_session(log, &session_info));

        if !session_info.path.join("profile").is_dir_async().await {
            return Err(ResumeSessionError {
                kind: ResumeSessionErrorKind::MissingProfile,
                session_id: session_id.into(),
            });
        }

        let firefox_path = session_info.path.join("firefox");
        let bin_path = firefox_path.join("firefox.exe");
        if !firefox_path.is_dir_async().await || !bin_path.is_file_async().await {
            return Err(ResumeSessionError {
                kind: ResumeSessionErrorKind::MissingFirefox,
                session_id: session_id.into(),
            });
        }

        drop(ScopeGuard::into_inner(cleanup));
        Ok(session_info)
    }

    async fn ensure_valid_profile_dir<'a>(
        &self,
        session_info: &SessionInfo<'a>,
    ) -> Result<PathBuf, io::Error> {
        let profile_path = session_info.path.join("profile");
        create_dir(&profile_path).await?;
        Ok(profile_path)
    }
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ResumeSessionErrorKind {
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
#[error("Invalid session ID `{}': {}", .session_id, .kind)]
pub struct ResumeSessionError {
    pub session_id: String,
    pub kind: ResumeSessionErrorKind,
}

#[derive(Debug, Error)]
pub enum NewSessionError {
    #[error(transparent)]
    Io(#[from] io::Error),

    #[error("Could not create a request directory after {} attempts", .0)]
    TooManyAttempts(u64),
}

/// Validate the given session ID is of the proper form.
fn validate_session_id(session_id: &str) -> bool {
    session_id.len() == REQUEST_ID_LEN && session_id.chars().all(|c| c.is_ascii_alphanumeric())
}

/// Synchronously cleanup a request given by the request info.
pub fn cleanup_session(log: slog::Logger, session_info: &SessionInfo<'_>) {
    // This must be performed synchronously because there is no async version of
    // the drop trait.
    //
    // A future could be spawned that would trigger when the guard goes out of
    // scope, but we cannot `await` its completion.
    //
    // Having a synchronous operation in the failure case seems like an okay
    // compromise.
    if let Err(e) = std::fs::remove_dir_all(&session_info.path) {
        error!(log, "Could not cleanup request"; "session_id" => %session_info.id, "error" => %e);
    }
}
