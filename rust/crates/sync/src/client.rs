//! Blocking HTTP client.

use crate::dto::{ArtifactDto, CreateJobRequest, JobDto, ListModelsResponse, LlmModelRefDto};
use crate::error::SyncError;

/// Клиент REST API backend (ADR-007).
#[derive(Debug, Clone)]
pub struct SyncClient {
    base_url: String,
    token: String,
    /// Один HTTP-клиент на всё время жизни, построенный при первом
    /// запросе.
    ///
    /// Каждый `build()` поднимает свой tokio-рантайм и паркует
    /// вызывающего, то есть поток встаёт ещё до отправки запроса
    /// (Epic 21). Строить его в конструкторе не годится:
    /// `MeetingCore::with_data_root` зовётся из `AppShellView.init` на
    /// главном потоке, так что парковка приходилась на запуск
    /// приложения — и приходилась даже когда backend не настроен и
    /// клиент не понадобится вовсе.
    ///
    /// `Arc` здесь несущий: каждый запрос из `ffi` работает с **копией**
    /// клиента, снятой из-под мьютекса ядра (`sync_client_snapshot`).
    /// Ячейка на экземпляр означала бы, что копия строит клиент заново,
    /// то есть ту же парковку на каждый вызов, от которой Epic 21 и
    /// уходил.
    ///
    /// Ошибка сборки хранится текстом и отдаётся при вызове: подменять
    /// её на «не настроено» нельзя.
    http: std::sync::Arc<std::sync::OnceLock<Result<reqwest::blocking::Client, String>>>,
}

impl SyncClient {
    pub fn new(base_url: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            base_url: trim_slash(base_url.into()),
            token: token.into(),
            http: std::sync::Arc::new(std::sync::OnceLock::new()),
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn is_configured(&self) -> bool {
        !self.base_url.is_empty()
    }

    pub fn health(&self) -> Result<(), SyncError> {
        self.ensure_configured()?;
        let response = self
            .http()?
            .get(format!("{}/health", self.base_url))
            .send()?;
        if !response.status().is_success() {
            return Err(SyncError::Http(
                response.status().as_u16(),
                response.text().unwrap_or_default(),
            ));
        }
        Ok(())
    }

    pub fn create_job(&self, request: &CreateJobRequest) -> Result<JobDto, SyncError> {
        self.ensure_configured()?;
        let response = self
            .http()?
            .post(format!("{}/v1/jobs", self.base_url))
            .header("Authorization", format!("Bearer {}", self.token))
            .json(request)
            .send()?;
        Self::parse_json(response)
    }

    pub fn get_job(&self, job_id: &str) -> Result<JobDto, SyncError> {
        self.ensure_configured()?;
        let response = self
            .http()?
            .get(format!("{}/v1/jobs/{job_id}", self.base_url))
            .header("Authorization", format!("Bearer {}", self.token))
            .send()?;
        Self::parse_json(response)
    }

    pub fn get_artifact(&self, artifact_id: &str) -> Result<ArtifactDto, SyncError> {
        self.ensure_configured()?;
        let response = self
            .http()?
            .get(format!("{}/v1/artifacts/{artifact_id}", self.base_url))
            .header("Authorization", format!("Bearer {}", self.token))
            .send()?;
        Self::parse_json(response)
    }

    /// Каталог доступных LLM-моделей (GET /v1/models).
    pub fn list_models(&self) -> Result<Vec<LlmModelRefDto>, SyncError> {
        self.ensure_configured()?;
        let response = self
            .http()?
            .get(format!("{}/v1/models", self.base_url))
            .header("Authorization", format!("Bearer {}", self.token))
            .send()?;
        let parsed: ListModelsResponse = Self::parse_json(response)?;
        Ok(parsed.models)
    }

    fn ensure_configured(&self) -> Result<(), SyncError> {
        if self.base_url.is_empty() {
            Err(SyncError::NotConfigured)
        } else {
            Ok(())
        }
    }

    fn http(&self) -> Result<&reqwest::blocking::Client, SyncError> {
        self.http
            .get_or_init(|| {
                reqwest::blocking::Client::builder()
                    .timeout(std::time::Duration::from_secs(10))
                    .build()
                    .map_err(|error| error.to_string())
            })
            .as_ref()
            .map_err(|error| SyncError::Transport(error.clone()))
    }

    fn parse_json<T: serde::de::DeserializeOwned>(
        response: reqwest::blocking::Response,
    ) -> Result<T, SyncError> {
        let status = response.status();
        let body = response.text().unwrap_or_default();
        if !status.is_success() {
            return Err(SyncError::Http(status.as_u16(), body));
        }
        serde_json::from_str(&body).map_err(SyncError::from)
    }
}

fn trim_slash(url: String) -> String {
    url.trim().trim_end_matches('/').to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::{JobKind, JobStatus};
    use mockito::Server;

    /// Конструктор зовётся при запуске приложения на главном потоке
    /// (`AppShellView.init` → `MeetingCore::with_data_root`), а сборка
    /// `reqwest` паркует вызывающего. Строить там нечего: backend может
    /// быть не настроен вовсе.
    #[test]
    fn new_does_not_build_the_http_client() {
        let client = SyncClient::new("http://127.0.0.1:1", "dev-token");
        assert!(
            client.http.get().is_none(),
            "клиент построен в конструкторе — главный поток паркуется на старте приложения"
        );
    }

    /// Каждый запрос из `ffi` идёт по копии, снятой из-под мьютекса
    /// ядра. Если ячейка своя у каждой копии, ленивость превращается в
    /// сборку клиента на каждый вызов — ровно та парковка, от которой
    /// Epic 21 уходил.
    #[test]
    fn clones_share_one_http_client() {
        let client = SyncClient::new("http://127.0.0.1:1", "dev-token");
        let snapshot = client.clone();

        // Запрос заведомо не дойдёт — важно, что он строит клиент.
        let _ = snapshot.health();

        assert!(
            client.http.get().is_some(),
            "копия построила собственный клиент — парковка вернулась на каждый запрос"
        );
    }

    #[test]
    fn health_ok_against_mock() {
        let mut server = Server::new();
        let _m = server
            .mock("GET", "/health")
            .with_status(200)
            .with_body(r#"{"status":"ok"}"#)
            .create();
        let client = SyncClient::new(server.url(), "dev-token");
        assert!(client.health().is_ok());
    }

    #[test]
    fn create_job_parses_response() {
        let mut server = Server::new();
        let _m = server
            .mock("POST", "/v1/jobs")
            .match_header("Authorization", "Bearer dev-token")
            .with_status(201)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                  "id":"j1",
                  "meeting_id":"m1",
                  "kind":"brief",
                  "status":"succeeded",
                  "error":null,
                  "artifact_ids":["a1"]
                }"#,
            )
            .create();
        let client = SyncClient::new(server.url(), "dev-token");
        let job = client
            .create_job(&CreateJobRequest {
                meeting_id: "m1".into(),
                kind: JobKind::Brief,
                primary_language: "ru".into(),
                allowed_languages: vec!["ru".into(), "en".into()],
                payload: None,
            })
            .unwrap();
        assert_eq!(job.id, "j1");
        assert_eq!(job.status, JobStatus::Succeeded);
        assert_eq!(job.artifact_ids, vec!["a1".to_string()]);
    }

    /// Клиент переживает первый запрос: он теперь общий на весь срок
    /// жизни, а не собирается заново под каждый вызов.
    #[test]
    fn the_same_client_serves_repeated_calls() {
        let mut server = Server::new();
        let _m = server
            .mock("GET", "/health")
            .with_status(200)
            .with_body(r#"{"status":"ok"}"#)
            .expect(2)
            .create();
        let client = SyncClient::new(server.url(), "dev-token");
        assert!(client.health().is_ok());
        assert!(client.health().is_ok());
        _m.assert();
    }

    #[test]
    fn not_configured_errors() {
        let client = SyncClient::new("", "");
        assert!(matches!(client.health(), Err(SyncError::NotConfigured)));
    }

    #[test]
    fn list_models_parses_catalog() {
        let mut server = Server::new();
        let _m = server
            .mock("GET", "/v1/models")
            .match_header("Authorization", "Bearer t")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"models":[{"provider_id":"home-llm","model":"m1","display_name":"One"}]}"#,
            )
            .create();
        let client = SyncClient::new(server.url(), "t");
        let models = client.list_models().unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].provider_id, "home-llm");
        assert_eq!(models[0].model, "m1");
        assert_eq!(models[0].display_name, "One");
    }
}
