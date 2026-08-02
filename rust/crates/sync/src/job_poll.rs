//! Ожидание завершения backend job и загрузка артефакта.

use crate::client::SyncClient;
use crate::dto::{ArtifactDto, CreateJobRequest, JobStatus};
use crate::error::SyncError;
use std::thread;
use std::time::Duration;

/// Создаёт job, опрашивает статус до успеха или таймаута, возвращает первый артефакт.
pub fn wait_for_job_artifact(
    client: &SyncClient,
    request: &CreateJobRequest,
    max_attempts: u32,
    poll_delay: Duration,
) -> Result<ArtifactDto, SyncError> {
    let mut job = client.create_job(request)?;
    if let Some(err) = job.error.as_ref().filter(|e| !e.is_empty()) {
        return Err(SyncError::Http(500, err.clone()));
    }
    if job.status == JobStatus::Failed {
        return Err(SyncError::Http(
            500,
            job.error.unwrap_or_else(|| "Backend job failed".into()),
        ));
    }

    let mut attempts = 0u32;
    while job.status != JobStatus::Succeeded {
        if attempts >= max_attempts {
            return Err(SyncError::Http(408, "Backend job timeout".into()));
        }
        if !poll_delay.is_zero() {
            thread::sleep(poll_delay);
        }
        job = client.get_job(&job.id)?;
        if let Some(err) = job.error.as_ref().filter(|e| !e.is_empty()) {
            return Err(SyncError::Http(500, err.clone()));
        }
        if job.status == JobStatus::Failed {
            return Err(SyncError::Http(
                500,
                job.error.unwrap_or_else(|| "Backend job failed".into()),
            ));
        }
        attempts += 1;
    }

    let artifact_id = job
        .artifact_ids
        .first()
        .ok_or_else(|| SyncError::Http(500, "Backend job has no artifacts".into()))?;
    client.get_artifact(artifact_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SyncClient;
    use crate::dto::{CreateJobRequest, JobKind};
    use mockito::Server;
    use std::time::Duration;

    fn request(meeting: &str) -> CreateJobRequest {
        CreateJobRequest {
            meeting_id: meeting.into(),
            kind: JobKind::Brief,
            primary_language: "ru".into(),
            allowed_languages: vec!["ru".into()],
            payload: None,
        }
    }

    #[test]
    fn immediate_success_fetches_artifact() {
        let mut server = Server::new();
        let _post = server
            .mock("POST", "/v1/jobs")
            .with_status(201)
            .with_body(r#"{"id":"j1","meeting_id":"m1","kind":"brief","status":"succeeded","error":null,"artifact_ids":["a1"]}"#)
            .create();
        let _art = server
            .mock("GET", "/v1/artifacts/a1")
            .with_status(200)
            .with_body(r##"{"id":"a1","kind":"brief","body_markdown":"# Stub brief","created_at":"2026-08-02T00:00:00Z"}"##)
            .create();
        let client = SyncClient::new(server.url(), "dev-token");
        let art = wait_for_job_artifact(&client, &request("m1"), 20, Duration::ZERO).unwrap();
        assert_eq!(art.body_markdown, "# Stub brief");
        assert_eq!(art.id, "a1");
    }

    #[test]
    fn failed_job_does_not_fetch_artifact() {
        let mut server = Server::new();
        let _post = server
            .mock("POST", "/v1/jobs")
            .with_status(201)
            .with_body(r#"{"id":"j1","meeting_id":"m1","kind":"brief","status":"failed","error":null,"artifact_ids":[]}"#)
            .create();
        let client = SyncClient::new(server.url(), "dev-token");
        let err = wait_for_job_artifact(&client, &request("m1"), 2, Duration::ZERO).unwrap_err();
        assert!(err.to_string().contains("failed") || err.to_string().contains("Backend"));
    }

    #[test]
    fn timeout_while_queued() {
        let mut server = Server::new();
        let _post = server
            .mock("POST", "/v1/jobs")
            .with_status(201)
            .with_body(r#"{"id":"j1","meeting_id":"m1","kind":"brief","status":"queued","error":null,"artifact_ids":[]}"#)
            .create();
        let _get = server
            .mock("GET", "/v1/jobs/j1")
            .with_status(200)
            .with_body(r#"{"id":"j1","meeting_id":"m1","kind":"brief","status":"queued","error":null,"artifact_ids":[]}"#)
            .expect_at_least(1)
            .create();
        let client = SyncClient::new(server.url(), "dev-token");
        let err = wait_for_job_artifact(&client, &request("m1"), 2, Duration::ZERO).unwrap_err();
        assert!(err.to_string().to_lowercase().contains("timeout"));
    }
}
