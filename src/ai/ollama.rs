//! The local Ollama backend, reached over loopback HTTP.
//!
//! Why the HTTP API and not the `ollama` command line: the API lets ARES ask
//! for deterministic output (temperature 0, fixed seed), constrain the reply to
//! a JSON shape, set the context window explicitly, and unload the model
//! afterwards. The command line can do none of these.
//!
//! The context window matters. A payload can be thousands of tokens, which is
//! more than Ollama's default window, and Ollama silently drops the *start* of
//! a prompt that does not fit, which is where the instructions are. So the
//! window is always set to fit the prompt and the reply.

use super::http::{post_json, HttpError};
use crate::redact::redact;
use serde_json::{json, Value};
use std::fmt;
use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

/// Tokens reserved for the model's reply.
pub const NUM_PREDICT: usize = 768;

/// Most of a reply ARES will read.
const MAX_RESPONSE: usize = 2 * 1024 * 1024;

/// Longest error text kept.
const MAX_ERROR_TEXT: usize = 200;

#[derive(Debug, Clone)]
pub struct Settings {
    pub model: String,
    pub port: u16,
    pub timeout: Duration,
    /// Ask Ollama to unload the model as soon as the answer is produced.
    pub unload_after: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum BackendError {
    InvalidModel(String),
    NotRunning(u16),
    ModelMissing(String),
    Timeout(Duration),
    /// The model stopped because it hit the reply-length limit.
    Truncated,
    /// Ollama replied, but not in a way ARES understands.
    BadResponse(String),
    /// Ollama reported an error.
    Failed(String),
}

impl fmt::Display for BackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BackendError::InvalidModel(m) => write!(f, "{m:?} is not a valid model name"),
            BackendError::NotRunning(port) => write!(
                f,
                "Ollama is not running on 127.0.0.1:{port}. Start it with `ollama serve` (or `sudo snap start ollama` if you installed the snap) and try again"
            ),
            BackendError::ModelMissing(m) => {
                write!(f, "the model '{m}' is not installed. Install it with: ollama pull {m}")
            }
            BackendError::Timeout(d) => write!(
                f,
                "Ollama did not answer within {} seconds. A first load or a larger model can be slow; raise [ai] timeout_secs",
                d.as_secs()
            ),
            BackendError::Truncated => write!(f, "the model's answer was cut off before it finished"),
            BackendError::BadResponse(m) => write!(f, "Ollama's reply was not understood: {m}"),
            BackendError::Failed(m) => write!(f, "Ollama reported an error: {m}"),
        }
    }
}

impl std::error::Error for BackendError {}

/// Model names look like `qwen2.5:1.5b`, `library/llama3.2:1b`, or
/// `hf.co/user/model:Q4_K_M`.
pub fn valid_model_name(s: &str) -> bool {
    (1..=100).contains(&s.len())
        && !s.starts_with(['-', '.', '/', ':'])
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ':' | '/' | '-'))
        && !s.contains("..")
}

/// A context window large enough for a prompt of about `prompt_tokens` plus
/// the reply, in multiples of 1024, never below 4096 or above 16384.
pub fn context_for(prompt_tokens: usize) -> usize {
    let need = prompt_tokens + NUM_PREDICT + 512;
    (((need + 1023) / 1024) * 1024).clamp(4096, 16384)
}

/// The JSON shape the model must reply in.
fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "statements": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "text": {"type": "string"},
                        "cites": {"type": "array", "items": {"type": "string"}}
                    },
                    "required": ["text", "cites"]
                }
            }
        },
        "required": ["statements"]
    })
}

fn request_body(s: &Settings, prompt: &str, format: Value) -> String {
    let tokens = (prompt.len() + 3) / 4;
    let mut body = json!({
        "model": s.model,
        "prompt": prompt,
        "stream": false,
        "format": format,
        "options": {
            "temperature": 0,
            "seed": 0,
            "num_ctx": context_for(tokens),
            "num_predict": NUM_PREDICT
        }
    });
    if s.unload_after {
        body["keep_alive"] = json!(0);
    }
    body.to_string()
}

fn error_text(body: &[u8]) -> String {
    let text = String::from_utf8_lossy(body);
    let msg = serde_json::from_str::<Value>(&text)
        .ok()
        .and_then(|v| v.get("error").and_then(Value::as_str).map(str::to_string))
        .unwrap_or_else(|| text.to_string());
    let one_line = msg.split_whitespace().collect::<Vec<_>>().join(" ");
    let clean = redact(&one_line);
    if clean.chars().count() > MAX_ERROR_TEXT {
        let cut: String = clean.chars().take(MAX_ERROR_TEXT).collect();
        format!("{cut}...")
    } else {
        clean
    }
}

pub struct Ollama {
    settings: Settings,
}

impl Ollama {
    /// Validates the model name; nothing is contacted.
    pub fn new(settings: Settings) -> Result<Ollama, BackendError> {
        if !valid_model_name(&settings.model) {
            return Err(BackendError::InvalidModel(settings.model));
        }
        Ok(Ollama { settings })
    }

    pub fn model(&self) -> &str {
        &self.settings.model
    }

    fn send(&self, prompt: &str, format: Value) -> Result<(u16, Vec<u8>), BackendError> {
        let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, self.settings.port));
        let body = request_body(&self.settings, prompt, format);
        match post_json(addr, "/api/generate", &body, self.settings.timeout, MAX_RESPONSE) {
            Ok(r) => Ok((r.status, r.body)),
            Err(HttpError::Connect(_)) => Err(BackendError::NotRunning(self.settings.port)),
            Err(HttpError::Timeout) => Err(BackendError::Timeout(self.settings.timeout)),
            Err(HttpError::TooLarge) => Err(BackendError::BadResponse("the reply was too large".into())),
            Err(HttpError::Malformed(m)) => Err(BackendError::BadResponse(m)),
            Err(HttpError::NotLoopback) | Err(HttpError::InvalidRequest(_)) => {
                Err(BackendError::Failed("the request was refused before it was sent".into()))
            }
            Err(HttpError::Io(e)) => Err(BackendError::Failed(redact(&e.to_string()))),
        }
    }

    /// Send `prompt` and return the model's raw reply text.
    pub fn complete(&self, prompt: &str) -> Result<String, BackendError> {
        let (mut status, mut body) = self.send(prompt, schema())?;

        // An older server may not accept a JSON schema for `format`. Ask again
        // for plain JSON mode.
        if status == 400 && error_text(&body).to_ascii_lowercase().contains("format") {
            let retry = self.send(prompt, json!("json"))?;
            status = retry.0;
            body = retry.1;
        }

        match status {
            200 => {
                let v: Value = serde_json::from_slice(&body)
                    .map_err(|_| BackendError::BadResponse("the reply is not valid JSON".into()))?;
                if v.get("done_reason").and_then(Value::as_str) == Some("length") {
                    return Err(BackendError::Truncated);
                }
                v.get("response")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .ok_or_else(|| BackendError::BadResponse("the reply has no text".into()))
            }
            404 if error_text(&body).to_ascii_lowercase().contains("not found") => {
                Err(BackendError::ModelMissing(self.settings.model.clone()))
            }
            other => Err(BackendError::Failed(format!("status {other}: {}", error_text(&body)))),
        }
    }
}
