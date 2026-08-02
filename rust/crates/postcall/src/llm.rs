use thiserror::Error;

/// Ошибки локального LLM-провайдера.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LlmError {
    #[error("LLM-клиент не настроен")]
    NotConfigured,
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
