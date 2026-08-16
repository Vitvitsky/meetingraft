use thiserror::Error;

/// Ошибки локального LLM-провайдера.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LlmError {
    #[error("LLM-клиент не настроен")]
    NotConfigured,
    #[error("LLM-провайдер вернул HTTP {status}: {body}")]
    Http { status: u16, body: String },
    #[error("LLM-провайдер вернул пустой ответ")]
    EmptyResponse,
    /// Модель не уложилась в отведённое время.
    ///
    /// Отдельно от [`LlmError::Transport`], потому что причина и лечение
    /// у них разные: транспорт — сеть или адрес, а это — модель, которой
    /// не хватило времени. Пока оба случая шли одной строкой «Ошибка
    /// транспорта LLM», человек с 12B-моделью читал про транспорт и шёл
    /// проверять адрес, который был в порядке.
    #[error(
        "Модель не ответила за {seconds} с. Локальная модель на длинной \
         расшифровке считает дольше: возьмите модель поменьше либо поднимите \
         потолок ожидания"
    )]
    Timeout { seconds: u64 },
    #[error("Ошибка транспорта LLM: {0}")]
    Transport(String),
}

/// Заменяемая граница для будущих Ollama, LM Studio или Gemma.
pub trait LlmClient: Send {
    fn complete(&self, system: &str, user: &str) -> Result<String, LlmError>;
}

/// Заглушка до подключения локального LLM-провайдера.
pub struct NullLlmClient;

impl LlmClient for NullLlmClient {
    fn complete(&self, _system: &str, _user: &str) -> Result<String, LlmError> {
        Err(LlmError::NotConfigured)
    }
}

#[cfg(test)]
mod tests {
    use super::{LlmClient, LlmError, NullLlmClient};

    #[test]
    fn null_client_reports_missing_configuration() {
        let result = NullLlmClient.complete("system", "user");

        assert_eq!(result, Err(LlmError::NotConfigured));
    }
}
