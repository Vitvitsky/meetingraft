use std::time::Duration;

use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};

use crate::{LlmClient, LlmError};

/// Сколько ждать ответа модели.
///
/// Запрос уходит без стриминга (`stream: false`), поэтому в этот потолок
/// должна уложиться **вся** сборка ответа, а не первый его байт: разбор
/// длинной расшифровки, генерация брифа целиком, а на первом вызове ещё
/// и загрузка весов в память.
///
/// Прежняя минута этого не выдерживала: 2026-08-14 бриф на локальной
/// 12B-модели падал «ошибкой транспорта» через минуту-другую, и человек
/// шёл проверять адрес и имя модели, которые были в порядке.
///
/// Десять минут выбраны по цене ошибки, а не по замеру — замера
/// скорости этой модели на этой машине ни у кого пока нет. Ошибиться
/// можно в две стороны, и они не равны: слишком строгий потолок **теряет
/// уже сделанную работу** и врёт о причине, слишком щедрый — заставляет
/// ждать зря отказавший сервер. Запрос при этом идёт вне мьютекса ядра
/// (Epic 21), то есть живые субтитры он не держит.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(600);

#[derive(Serialize)]
struct Message<'a> {
    role: &'static str,
    content: &'a str,
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
    messages: [Message<'a>; 2],
}

#[derive(Deserialize)]
struct ResponseMessage {
    content: Option<String>,
}

#[derive(Deserialize)]
struct OllamaResponse {
    message: Option<ResponseMessage>,
}

#[derive(Deserialize)]
struct Choice {
    message: Option<ResponseMessage>,
}

#[derive(Deserialize)]
struct OpenAiResponse {
    #[serde(default)]
    choices: Vec<Choice>,
}

struct HttpClient {
    base_url: String,
    model: String,
    timeout: Duration,
    client: Result<Client, String>,
}

impl HttpClient {
    fn new(base_url: impl Into<String>, model: impl Into<String>) -> Self {
        Self::with_timeout(base_url, model, REQUEST_TIMEOUT)
    }

    fn with_timeout(
        base_url: impl Into<String>,
        model: impl Into<String>,
        timeout: Duration,
    ) -> Self {
        let base_url = base_url.into().trim().trim_end_matches('/').to_owned();
        let model = model.into().trim().to_owned();
        let client = Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|error| error.to_string());

        Self {
            base_url,
            model,
            timeout,
            client,
        }
    }

    fn complete(
        &self,
        path: &str,
        stream: Option<bool>,
        system: &str,
        user: &str,
    ) -> Result<String, LlmError> {
        if self.base_url.is_empty() || self.model.is_empty() {
            return Err(LlmError::NotConfigured);
        }

        let client = self
            .client
            .as_ref()
            .map_err(|error| LlmError::Transport(error.clone()))?;
        let request = ChatRequest {
            model: &self.model,
            stream,
            messages: [
                Message {
                    role: "system",
                    content: system,
                },
                Message {
                    role: "user",
                    content: user,
                },
            ],
        };
        let response = client
            .post(format!("{}{}", self.base_url, path))
            .json(&request)
            .send()
            .map_err(|error| self.failure(&error))?;
        let status = response.status();
        let body = response.text().map_err(|error| self.failure(&error))?;

        if !status.is_success() {
            return Err(LlmError::Http {
                status: status.as_u16(),
                body,
            });
        }

        Ok(body)
    }

    /// Отличить «модель не успела» от «до сервера не доехало».
    ///
    /// Разница видна только здесь: выше по стеку обе выглядят одинаково
    /// — ошибкой запроса, — и потому обе годами ехали одной строкой про
    /// транспорт.
    fn failure(&self, error: &reqwest::Error) -> LlmError {
        if error.is_timeout() {
            return LlmError::Timeout {
                seconds: self.timeout.as_secs(),
            };
        }
        LlmError::Transport(error.to_string())
    }
}

/// Клиент нативного Ollama Chat API.
pub struct OllamaNativeClient {
    http: HttpClient,
}

impl OllamaNativeClient {
    pub fn new(base_url: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            http: HttpClient::new(base_url, model),
        }
    }

    /// Тот же клиент со своим потолком ожидания.
    ///
    /// Нужен, чтобы отказ по времени можно было проверить тестом за
    /// секунду, а не за десять минут. Умолчание — [`REQUEST_TIMEOUT`].
    pub fn with_timeout(
        base_url: impl Into<String>,
        model: impl Into<String>,
        timeout: Duration,
    ) -> Self {
        Self {
            http: HttpClient::with_timeout(base_url, model, timeout),
        }
    }
}

impl LlmClient for OllamaNativeClient {
    fn complete(&self, system: &str, user: &str) -> Result<String, LlmError> {
        let body = self.http.complete("/api/chat", Some(false), system, user)?;
        let response: OllamaResponse =
            serde_json::from_str(&body).map_err(|error| LlmError::Transport(error.to_string()))?;

        non_empty_content(response.message.and_then(|message| message.content))
    }
}

/// Клиент OpenAI-совместимого Chat Completions API.
pub struct OpenAiCompatLlmClient {
    http: HttpClient,
}

impl OpenAiCompatLlmClient {
    pub fn new(base_url: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            http: HttpClient::new(base_url, model),
        }
    }

    /// Тот же клиент со своим потолком ожидания.
    pub fn with_timeout(
        base_url: impl Into<String>,
        model: impl Into<String>,
        timeout: Duration,
    ) -> Self {
        Self {
            http: HttpClient::with_timeout(base_url, model, timeout),
        }
    }
}

impl LlmClient for OpenAiCompatLlmClient {
    fn complete(&self, system: &str, user: &str) -> Result<String, LlmError> {
        let body = self
            .http
            .complete("/v1/chat/completions", None, system, user)?;
        let response: OpenAiResponse =
            serde_json::from_str(&body).map_err(|error| LlmError::Transport(error.to_string()))?;
        let content = response
            .choices
            .into_iter()
            .next()
            .and_then(|choice| choice.message)
            .and_then(|message| message.content);

        non_empty_content(content)
    }
}

fn non_empty_content(content: Option<String>) -> Result<String, LlmError> {
    let content = content.ok_or(LlmError::EmptyResponse)?;
    let content = content.trim();
    if content.is_empty() {
        return Err(LlmError::EmptyResponse);
    }

    Ok(content.to_owned())
}

#[cfg(test)]
mod tests {
    use mockito::{Matcher, Server};

    use std::time::Duration;

    use crate::{LlmClient, LlmError};

    use super::{OllamaNativeClient, OpenAiCompatLlmClient};

    /// Сервер, который принял соединение и молчит.
    ///
    /// Именно этот случай ловил прежний потолок: модель считает, сокет
    /// открыт, ответа ещё нет.
    fn silent_server() -> (String, std::thread::JoinHandle<()>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("сокет");
        let address = listener.local_addr().expect("адрес");
        let handle = std::thread::spawn(move || {
            // Соединение держится открытым: закрой его — и клиент получит
            // разрыв, то есть **другую** ошибку, и тест проверял бы не то.
            let _accepted = listener.incoming().next();
            std::thread::sleep(Duration::from_secs(3));
        });
        (format!("http://{address}"), handle)
    }

    #[test]
    fn a_silent_model_is_a_timeout_not_a_transport_error() {
        let (base_url, _server) = silent_server();

        let error = OllamaNativeClient::with_timeout(base_url, "gemma", Duration::from_secs(1))
            .complete("sys", "user")
            .expect_err("молчащий сервер обязан кончиться отказом");

        assert_eq!(
            error,
            LlmError::Timeout { seconds: 1 },
            "отказ по времени не должен выглядеть отказом транспорта: человек \
             пойдёт проверять адрес, который в порядке"
        );
    }

    /// Заведомо положительный случай к предыдущему тесту.
    ///
    /// Тот же короткий потолок против отвечающего сервера: без этой
    /// проверки «отказ по времени» мог бы означать, что с секундным
    /// потолком не проходит ничто вообще.
    #[test]
    fn the_same_short_ceiling_lets_a_fast_answer_through() {
        let mut server = Server::new();
        let _mock = server
            .mock("POST", "/api/chat")
            .with_status(200)
            .with_body(r#"{"message":{"content":"успел"}}"#)
            .create();

        let answer =
            OllamaNativeClient::with_timeout(server.url(), "gemma", Duration::from_secs(1))
                .complete("sys", "user")
                .expect("быстрый ответ обязан пройти");

        assert_eq!(answer, "успел");
    }

    #[test]
    fn ollama_native_parses_message_content() {
        let mut server = Server::new();
        let _mock = server
            .mock("POST", "/api/chat")
            .match_body(Matcher::PartialJson(serde_json::json!({
                "model": "gemma2",
                "stream": false,
                "messages": [
                    {"role": "system", "content": "sys"},
                    {"role": "user", "content": "user"}
                ]
            })))
            .with_status(200)
            .with_body(
                r##"{"message":{"role":"assistant","content":"# Brief from Ollama"},"done":true}"##,
            )
            .create();
        let client = OllamaNativeClient::new(server.url(), "gemma2");

        let output = client.complete("sys", "user").unwrap();

        assert_eq!(output, "# Brief from Ollama");
    }

    #[test]
    fn openai_compat_parses_choices_content() {
        let mut server = Server::new();
        let _mock = server
            .mock("POST", "/v1/chat/completions")
            .match_body(Matcher::PartialJson(serde_json::json!({
                "model": "gemma2",
                "messages": [
                    {"role": "system", "content": "sys"},
                    {"role": "user", "content": "user"}
                ]
            })))
            .with_status(200)
            .with_body(
                r##"{"choices":[{"message":{"role":"assistant","content":"# Brief compat"}}]}"##,
            )
            .create();
        let client = OpenAiCompatLlmClient::new(format!("{}/", server.url()), "gemma2");

        let output = client.complete("sys", "user").unwrap();

        assert_eq!(output, "# Brief compat");
    }

    #[test]
    fn http_error_maps_to_llm_http() {
        let mut server = Server::new();
        let _mock = server
            .mock("POST", "/api/chat")
            .with_status(500)
            .with_body("provider unavailable")
            .create();
        let client = OllamaNativeClient::new(server.url(), "gemma2");

        let result = client.complete("sys", "user");

        assert_eq!(
            result,
            Err(LlmError::Http {
                status: 500,
                body: "provider unavailable".to_owned(),
            })
        );
    }

    #[test]
    fn empty_content_is_empty_response() {
        let mut server = Server::new();
        let _mock = server
            .mock("POST", "/v1/chat/completions")
            .with_status(200)
            .with_body(r#"{"choices":[{"message":{"role":"assistant","content":"  "}}]}"#)
            .create();
        let client = OpenAiCompatLlmClient::new(server.url(), "gemma2");

        let result = client.complete("sys", "user");

        assert_eq!(result, Err(LlmError::EmptyResponse));
    }

    #[test]
    fn empty_base_is_not_configured() {
        let client = OllamaNativeClient::new("", "gemma2");

        let result = client.complete("sys", "user");

        assert_eq!(result, Err(LlmError::NotConfigured));
    }

    #[test]
    fn empty_model_is_not_configured() {
        let client = OpenAiCompatLlmClient::new("http://localhost:1234", "");

        let result = client.complete("sys", "user");

        assert_eq!(result, Err(LlmError::NotConfigured));
    }
}
