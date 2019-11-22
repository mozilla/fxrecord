// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::error::Error;
use std::io;
use std::path::{Path, PathBuf};

use chrono::prelude::*;
use derive_more::Display;
use futures::prelude::*;
use futures::try_join;
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use tokio::fs::File;
use tokio::prelude::*;

/// The name of the artifact containing the result of a build job.
pub const BUILD_ARTIFACT_NAME: &str = "public/build/target.zip";

#[derive(Deserialize, Serialize)]
pub struct Artifact {
    pub name: String,
    pub expires: DateTime<Utc>,
}

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

    #[display(fmt = "artifact expired on {}", _0)]
    Expired(DateTime<Utc>),

    #[display(fmt = "no build artifact found")]
    NotFound,

    #[display(fmt = "an error occurred while downloading the artifact: {}", _0)]
    DownloadArtifact(reqwest::Error),
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
            TaskclusterError::Expired(..) => None,
            TaskclusterError::NotFound => None,
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
        let artifacts = self.list_artifacts(task_id).await?;
        for artifact in artifacts {
            if artifact.name == BUILD_ARTIFACT_NAME {
                if artifact.expires <= Utc::now() {
                    return Err(TaskclusterError::Expired(artifact.expires));
                }

                let path = download_dir.join("firefox.zip");

                self.download_artifact(task_id, &artifact.name, &path)
                    .await?;

                return Ok(path);
            }
        }

        Err(TaskclusterError::NotFound)
    }

    async fn list_artifacts(&mut self, task_id: &str) -> Result<Vec<Artifact>, TaskclusterError> {
        let url = self
            .queue_url
            .join(&format!("task/{}/artifacts", task_id))?;

        self.client
            .get(url)
            .send()
            .await
            .map_err(TaskclusterError::ListArtifacts)?
            .json::<ArtifactsResponse>()
            .await
            .map_err(TaskclusterError::ListArtifacts)
            .map(|rsp| rsp.artifacts)
    }

    async fn download_artifact(
        &mut self,
        task_id: &str,
        artifact_name: &str,
        path: &Path,
    ) -> Result<(), TaskclusterError> {
        let url = self
            .queue_url
            .join(&format!("task/{}/artifacts/{}", task_id, artifact_name))?;

        let (mut file, mut request) = try_join!(
            File::create(path).map_err(TaskclusterError::Io),
            self.client
                .get(url)
                .send()
                .map_err(TaskclusterError::DownloadArtifact),
        )?;

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

        Ok(())
    }
}

/// A response from the artifacts API.
///
/// This is only used to destructure the API response into `Vec<Artifact>`.
#[derive(Deserialize, Serialize)]
pub struct ArtifactsResponse {
    pub artifacts: Vec<Artifact>,
}

#[cfg(test)]
mod test {
    use std::env::current_dir;

    use assert_matches::assert_matches;
    use tempfile::TempDir;

    use super::ArtifactsResponse;
    use crate::taskcluster::*;

    fn test_queue_url() -> Url {
        Url::parse(&mockito::server_url())
            .unwrap()
            .join("/api/queue/v1/")
            .unwrap()
    }

    #[tokio::test]
    async fn test_firefox_ci() {
        let zip_path = current_dir()
            .unwrap()
            .parent()
            .unwrap()
            .join("test")
            .join("test.zip");

        let list_rsp = mockito::mock("GET", "/api/queue/v1/task/foo/artifacts")
            .with_body(
                serde_json::to_string(&ArtifactsResponse {
                    artifacts: vec![Artifact {
                        name: BUILD_ARTIFACT_NAME.into(),
                        expires: Utc::now()
                            .checked_add_signed(chrono::Duration::seconds(3600))
                            .unwrap(),
                    }],
                })
                .unwrap(),
            )
            .create();

        let artifact_rsp = mockito::mock(
            "GET",
            &*format!("/api/queue/v1/task/foo/artifacts/{}", BUILD_ARTIFACT_NAME),
        )
        .with_body_from_file(zip_path)
        .create();

        let download_dir = TempDir::new().unwrap();

        Taskcluster::with_queue_url(test_queue_url())
            .download_build_artifact("foo", download_dir.path())
            .await
            .unwrap();

        list_rsp.assert();
        artifact_rsp.assert();
    }

    #[tokio::test]
    async fn test_firefox_ci_not_found() {
        let list_rsp = mockito::mock("GET", "/api/queue/v1/task/foo/artifacts")
            .with_body(serde_json::to_string(&ArtifactsResponse { artifacts: vec![] }).unwrap())
            .create();

        let download_dir = TempDir::new().unwrap();

        assert_matches!(
            Taskcluster::with_queue_url(test_queue_url())
                .download_build_artifact("foo", download_dir.path())
                .await
                .unwrap_err(),
            TaskclusterError::NotFound
        );

        list_rsp.assert();
    }

    #[tokio::test]
    async fn test_firefox_ci_expired() {
        let expiry = Utc::now()
            .checked_sub_signed(chrono::Duration::days(1))
            .unwrap();
        let list_rsp = mockito::mock("GET", "/api/queue/v1/task/foo/artifacts")
            .with_body(
                serde_json::to_string(&ArtifactsResponse {
                    artifacts: vec![Artifact {
                        name: BUILD_ARTIFACT_NAME.into(),
                        expires: expiry,
                    }],
                })
                .unwrap(),
            )
            .create();

        let download_dir = TempDir::new().unwrap();

        assert_matches!(
            Taskcluster::with_queue_url(test_queue_url())
                .download_build_artifact("foo", download_dir.path())
                .await
                .unwrap_err(),
            TaskclusterError::Expired(e) => {
                assert_eq!(e, expiry);
            }
        );

        list_rsp.assert();
    }
}
