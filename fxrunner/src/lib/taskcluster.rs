// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::error::Error;
use std::io;
use std::path::{Path, PathBuf};

use derive_more::Display;
use futures::prelude::*;
use futures::try_join;
use reqwest::{Client, StatusCode, Url};
use tokio::fs::File;
use tokio::prelude::*;

/// The name of the artifact containing the result of a build job.
pub const BUILD_ARTIFACT_NAME: &str = "public/build/target.zip";

/// An error from Taskcluster
#[derive(Debug, Display)]
pub enum TaskclusterError {
    /// An
    #[display(fmt = "IO error: {}", _0)]
    Io(io::Error),

    #[display(fmt = "could not parse URL: {}", _0)]
    UrlParse(url::ParseError),

    #[display(fmt = "could not list artifacts: {}", _0)]
    ListArtifacts(reqwest::Error),

    #[display(fmt = "an error occurred while downloading the artifact: {}", _0)]
    DownloadArtifact(reqwest::Error),

    #[display(fmt = "an error occurred while downloading the artifact: {}", _0)]
    StatusError(StatusCode),
}

impl From<io::Error> for TaskclusterError {
    fn from(e: io::Error) -> Self {
        TaskclusterError::Io(e)
    }
}

impl From<url::ParseError> for TaskclusterError {
    fn from(e: url::ParseError) -> Self {
        TaskclusterError::UrlParse(e)
    }
}

impl Error for TaskclusterError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            TaskclusterError::Io(ref e) => Some(e),
            TaskclusterError::UrlParse(ref e) => Some(e),
            TaskclusterError::ListArtifacts(ref e) => Some(e),
            TaskclusterError::DownloadArtifact(ref e) => Some(e),
            TaskclusterError::StatusError(..) => None,
        }
    }
}

/// An API client to download Taskcluster build artifacts.
pub struct Taskcluster {
    /// The reqwest Client used for all requests.
    client: Client,

    /// The URL for the Taskcluster Queue API.
    queue_url: Url,
}

impl Default for Taskcluster {
    fn default() -> Self {
        Taskcluster {
            queue_url: Url::parse("https://firefox-ci-tc.services.mozilla.com/api/queue/v1/")
                .unwrap(),
            client: Client::new(),
        }
    }
}

impl Taskcluster {
    pub fn with_queue_url(queue_url: Url) -> Self {
        Taskcluster {
            client: Client::new(),
            queue_url,
        }
    }

    /// Download the build artifact from a Taskcluster task.
    pub async fn download_build_artifact(
        &mut self,
        task_id: &str,
        download_dir: &Path,
    ) -> Result<PathBuf, TaskclusterError> {
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
            .map_err(TaskclusterError::DownloadArtifact)?;

        if !request.status().is_success() {
            return Err(TaskclusterError::StatusError(request.status()));
        }

        let mut file = File::create(&path).await.map_err(TaskclusterError::Io)?;

        // Stream the first chunk ...
        let mut chunk = request
            .chunk()
            .await
            .map_err(TaskclusterError::DownloadArtifact)?;

        // Then write the previous chunk to disk while streaming the next chunk.
        while let Some(content) = chunk {
            chunk = try_join!(
                request.chunk().map_err(TaskclusterError::DownloadArtifact),
                file.write_all(&content).map_err(TaskclusterError::Io),
            )?
            .0;
        }

        Ok(path)
    }
}

#[cfg(test)]
mod test {
    use assert_matches::assert_matches;
    use reqwest::StatusCode;
    use tempfile::TempDir;

    use crate::taskcluster::*;

    fn test_queue_url() -> Url {
        Url::parse(&mockito::server_url())
            .unwrap()
            .join("/api/queue/v1/")
            .unwrap()
    }

    #[tokio::test]
    async fn test_firefox_ci() {
        let artifact_rsp = mockito::mock(
            "GET",
            &*format!("/api/queue/v1/task/foo/artifacts/{}", BUILD_ARTIFACT_NAME),
        )
        .with_body("foo")
        .create();

        let download_dir = TempDir::new().unwrap();

        let download = Taskcluster::with_queue_url(test_queue_url())
            .download_build_artifact("foo", download_dir.path())
            .await
            .unwrap();

        let mut buf = String::new();
        File::open(&download)
            .await
            .unwrap()
            .read_to_string(&mut buf)
            .await
            .unwrap();

        assert_eq!(buf, "foo");

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
            Taskcluster::with_queue_url(test_queue_url())
                .download_build_artifact("foo", download_dir.path())
                .await
                .unwrap_err(),
            TaskclusterError::StatusError(StatusCode::NOT_FOUND)
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
            Taskcluster::with_queue_url(test_queue_url())
                .download_build_artifact("foo", download_dir.path())
                .await
                .unwrap_err(),
            TaskclusterError::StatusError(StatusCode::SERVICE_UNAVAILABLE)
        );

        artifact_rsp.assert();
    }
}
