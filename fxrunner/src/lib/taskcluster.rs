// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::error::Error;
use std::fmt::Debug;
use std::io;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use futures::prelude::*;
use futures::try_join;
use reqwest::{Client, StatusCode, Url};
use thiserror::Error;
use tokio::fs::File;
use tokio::prelude::*;

/// The name of the artifact containing the result of a build job.
pub const BUILD_ARTIFACT_NAME: &str = "public/build/target.zip";

/// An error from Firefox CI.
#[derive(Debug, Error)]
pub enum FirefoxCiError {
    /// An
    #[error("IO error: {}", .0)]
    Io(#[from] io::Error),

    #[error("could not parse URL: {}", .0)]
    UrlParse(#[from] url::ParseError),

    #[error("could not list artifacts: {}", .0)]
    ListArtifacts(#[source] reqwest::Error),

    #[error("an error occurred while downloading the artifact: {}", .0)]
    DownloadArtifact(#[source] reqwest::Error),

    #[error("an error occurred while downloading the artifact: {}", .0)]
    StatusError(StatusCode),
}

#[async_trait]
pub trait Taskcluster: Debug {
    type Error: Error + 'static;

    async fn download_build_artifact(
        &mut self,
        task_id: &str,
        download_dir: &Path,
    ) -> Result<PathBuf, Self::Error>;
}

/// An API client to download Taskcluster build artifacts.
#[derive(Debug)]
pub struct FirefoxCi {
    /// The reqwest Client used for all requests.
    client: Client,

    /// The URL for the Taskcluster Queue API.
    queue_url: Url,
}

impl Default for FirefoxCi {
    fn default() -> Self {
        FirefoxCi {
            queue_url: Url::parse("https://firefox-ci-tc.services.mozilla.com/api/queue/v1/")
                .unwrap(),
            client: Client::new(),
        }
    }
}

impl FirefoxCi {
    #[cfg(test)]
    pub(crate) fn with_queue_url(queue_url: Url) -> Self {
        FirefoxCi {
            client: Client::new(),
            queue_url,
        }
    }
}

#[async_trait]
impl Taskcluster for FirefoxCi {
    type Error = FirefoxCiError;

    /// Download the build artifact from a Taskcluster task.
    async fn download_build_artifact(
        &mut self,
        task_id: &str,
        download_dir: &Path,
    ) -> Result<PathBuf, FirefoxCiError> {
        let url = self.queue_url.join(&format!(
            "task/{}/artifacts/{}",
            task_id, BUILD_ARTIFACT_NAME
        ))?;

        let path = download_dir.join("firefox.zip");

        let mut request = self
            .client
            .get(url)
            .send()
            .await
            .map_err(FirefoxCiError::DownloadArtifact)?;

        if !request.status().is_success() {
            return Err(FirefoxCiError::StatusError(request.status()));
        }

        let mut file = File::create(&path).await.map_err(FirefoxCiError::Io)?;

        // Stream the first chunk ...
        let mut chunk = request
            .chunk()
            .await
            .map_err(FirefoxCiError::DownloadArtifact)?;

        // Then write the previous chunk to disk while streaming the next chunk.
        while let Some(content) = chunk {
            chunk = try_join!(
                request.chunk().map_err(FirefoxCiError::DownloadArtifact),
                file.write_all(&content).map_err(FirefoxCiError::Io),
            )?
            .0;
        }

        Ok(path)
    }
}

#[cfg(test)]
mod test {
    use std::env::current_dir;

    use assert_matches::assert_matches;
    use reqwest::StatusCode;
    use tempfile::TempDir;

    use super::*;

    fn firefox_ci() -> FirefoxCi {
        FirefoxCi::with_queue_url(
            Url::parse(&mockito::server_url())
                .unwrap()
                .join("/api/queue/v1/")
                .unwrap(),
        )
    }

    #[tokio::test]
    async fn test_firefox_ci() {
        let zip_path = current_dir()
            .unwrap()
            .parent()
            .unwrap()
            .join("test")
            .join("test.zip");

        let artifact_rsp = mockito::mock(
            "GET",
            &*format!("/api/queue/v1/task/foo/artifacts/{}", BUILD_ARTIFACT_NAME),
        )
        .with_body_from_file(zip_path)
        .create();

        let download_dir = TempDir::new().unwrap();

        firefox_ci()
            .download_build_artifact("foo", download_dir.path())
            .await
            .unwrap();

        artifact_rsp.assert();
    }

    #[tokio::test]
    async fn test_firefox_ci_404() {
        let artifact_rsp = mockito::mock(
            "GET",
            &*format!("/api/queue/v1/task/foo/artifacts/{}", BUILD_ARTIFACT_NAME),
        )
        .with_status(404)
        .with_body("not found")
        .create();

        let download_dir = TempDir::new().unwrap();

        assert_matches!(
            firefox_ci()
                .download_build_artifact("foo", download_dir.path())
                .await
                .unwrap_err(),
            FirefoxCiError::StatusError(StatusCode::NOT_FOUND)
        );

        artifact_rsp.assert();
    }

    #[tokio::test]
    async fn test_firefox_ci_503() {
        let artifact_rsp = mockito::mock(
            "GET",
            &*format!("/api/queue/v1/task/foo/artifacts/{}", BUILD_ARTIFACT_NAME),
        )
        .with_status(503)
        .with_body("not found")
        .create();

        let download_dir = TempDir::new().unwrap();

        assert_matches!(
            firefox_ci()
                .download_build_artifact("foo", download_dir.path())
                .await
                .unwrap_err(),
            FirefoxCiError::StatusError(StatusCode::SERVICE_UNAVAILABLE)
        );

        artifact_rsp.assert();
    }
}
