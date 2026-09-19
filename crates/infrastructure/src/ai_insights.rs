//! On-demand AI portfolio insights — Anthropic, OpenAI, or Gemini,
//! whichever the user has configured a key for and explicitly chooses to
//! use. Deliberately NOT automatic: every call here happens because the
//! user clicked a button after seeing exactly what would be sent, matching
//! the "explicit consent before cloud calls" principle this project's own
//! original design laid out (see the Settings screen's original AI
//! Assistant placeholder, now replaced by this real implementation).
//!
//! HONESTY NOTE: unlike Alpha Vantage (live-verified by the user with a
//! real key before this was built) or Upstox (built against Upstox's own
//! published docs), the exact request/response shapes below are built
//! from well-established, stable API conventions for each of these three
//! vendors — not live-verified from this sandbox, since no keys were
//! available to test with while writing this. Model names are stored as
//! configurable settings with reasonable defaults rather than hardcoded,
//! specifically because model availability changes over time and a stale
//! hardcoded model string would be the most likely single point of
//! failure here.
//!
//! This module only ever returns the model's raw text response — it does
//! NOT interpret, execute, or act on anything the model says. Whatever
//! comes back is displayed to the user as-is, framed clearly as
//! informational analysis, not investment advice (the calling UI code is
//! responsible for that framing, not this module).

use reqwest::Client;
use serde::Deserialize;
use serde_json::json;

pub const ANTHROPIC_API_KEY_SETTING: &str = "anthropic_api_key";
pub const OPENAI_API_KEY_SETTING: &str = "openai_api_key";
pub const GEMINI_API_KEY_SETTING: &str = "gemini_api_key";
pub const ANTHROPIC_MODEL_SETTING: &str = "anthropic_model";
pub const OPENAI_MODEL_SETTING: &str = "openai_model";
pub const GEMINI_MODEL_SETTING: &str = "gemini_model";

pub const DEFAULT_ANTHROPIC_MODEL: &str = "claude-sonnet-5";
pub const DEFAULT_OPENAI_MODEL: &str = "gpt-4o";
pub const DEFAULT_GEMINI_MODEL: &str = "gemini-2.0-flash";

#[derive(Debug, thiserror::Error)]
pub enum AiError {
    #[error("request failed: {0}")]
    RequestFailed(String),
    #[error("unexpected response shape: {0}")]
    UnexpectedResponse(String),
    #[error("{provider} returned an error: {message}")]
    ProviderError { provider: String, message: String },
}

pub struct AiInsightsClient {
    http: Client,
}

impl AiInsightsClient {
    pub fn new() -> Self {
        Self { http: Client::new() }
    }

    pub async fn generate_insights(&self, provider: &str, api_key: &str, model: &str, prompt: &str) -> Result<String, AiError> {
        match provider {
            "anthropic" => self.call_anthropic(api_key, model, prompt).await,
            "openai" => self.call_openai(api_key, model, prompt).await,
            "gemini" => self.call_gemini(api_key, model, prompt).await,
            other => Err(AiError::RequestFailed(format!("unknown provider '{other}' — expected anthropic, openai, or gemini"))),
        }
    }

    async fn call_anthropic(&self, api_key: &str, model: &str, prompt: &str) -> Result<String, AiError> {
        let response = self
            .http
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&json!({
                "model": model,
                "max_tokens": 1500,
                "messages": [{"role": "user", "content": prompt}],
            }))
            .send()
            .await
            .map_err(|e| AiError::RequestFailed(e.to_string()))?;

        let body: AnthropicResponse = response
            .json()
            .await
            .map_err(|e| AiError::UnexpectedResponse(format!("couldn't parse Anthropic response: {e}")))?;

        if let Some(err) = body.error {
            return Err(AiError::ProviderError { provider: "Anthropic".to_string(), message: err.message });
        }
        body.content
            .into_iter()
            .find_map(|block| block.text)
            .ok_or_else(|| AiError::UnexpectedResponse("no text content in Anthropic response".to_string()))
    }

    async fn call_openai(&self, api_key: &str, model: &str, prompt: &str) -> Result<String, AiError> {
        let response = self
            .http
            .post("https://api.openai.com/v1/chat/completions")
            .header("Authorization", format!("Bearer {api_key}"))
            .header("content-type", "application/json")
            .json(&json!({
                "model": model,
                "messages": [{"role": "user", "content": prompt}],
            }))
            .send()
            .await
            .map_err(|e| AiError::RequestFailed(e.to_string()))?;

        let body: OpenAiResponse = response
            .json()
            .await
            .map_err(|e| AiError::UnexpectedResponse(format!("couldn't parse OpenAI response: {e}")))?;

        if let Some(err) = body.error {
            return Err(AiError::ProviderError { provider: "OpenAI".to_string(), message: err.message });
        }
        body.choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .ok_or_else(|| AiError::UnexpectedResponse("no choices in OpenAI response".to_string()))
    }

    async fn call_gemini(&self, api_key: &str, model: &str, prompt: &str) -> Result<String, AiError> {
        let url = format!("https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent?key={api_key}");
        let response = self
            .http
            .post(&url)
            .header("content-type", "application/json")
            .json(&json!({
                "contents": [{"parts": [{"text": prompt}]}],
            }))
            .send()
            .await
            .map_err(|e| AiError::RequestFailed(e.to_string()))?;

        let body: GeminiResponse = response
            .json()
            .await
            .map_err(|e| AiError::UnexpectedResponse(format!("couldn't parse Gemini response: {e}")))?;

        if let Some(err) = body.error {
            return Err(AiError::ProviderError { provider: "Gemini".to_string(), message: err.message });
        }
        body.candidates
            .into_iter()
            .next()
            .and_then(|c| c.content.parts.into_iter().next())
            .map(|p| p.text)
            .ok_or_else(|| AiError::UnexpectedResponse("no candidates in Gemini response".to_string()))
    }
}

impl Default for AiInsightsClient {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicBlock>,
    error: Option<AnthropicError>,
}
#[derive(Deserialize)]
struct AnthropicBlock {
    text: Option<String>,
}
#[derive(Deserialize)]
struct AnthropicError {
    message: String,
}

#[derive(Deserialize)]
struct OpenAiResponse {
    choices: Vec<OpenAiChoice>,
    error: Option<OpenAiError>,
}
#[derive(Deserialize)]
struct OpenAiChoice {
    message: OpenAiMessage,
}
#[derive(Deserialize)]
struct OpenAiMessage {
    content: String,
}
#[derive(Deserialize)]
struct OpenAiError {
    message: String,
}

#[derive(Deserialize)]
struct GeminiResponse {
    candidates: Vec<GeminiCandidate>,
    error: Option<GeminiError>,
}
#[derive(Deserialize)]
struct GeminiCandidate {
    content: GeminiContent,
}
#[derive(Deserialize)]
struct GeminiContent {
    parts: Vec<GeminiPart>,
}
#[derive(Deserialize)]
struct GeminiPart {
    text: String,
}
#[derive(Deserialize)]
struct GeminiError {
    message: String,
}

/// Builds the actual prompt sent to whichever provider is chosen — kept as
/// a pure function, separate from any network code, so it's easy to see
/// and test exactly what leaves the app. Deliberately structured data, not
/// free-form phrasing, to keep the model's response grounded in the real
/// numbers rather than inviting speculation.
pub fn build_portfolio_prompt(portfolio_name: &str, holdings_summary: &str, sector_allocation: &str, xirr_pct: Option<f64>) -> String {
    let xirr_line = xirr_pct.map(|x| format!("Portfolio XIRR: {x:.2}%\n")).unwrap_or_default();
    format!(
        "You are analyzing a stock portfolio named \"{portfolio_name}\" for informational purposes only. \
         This is not a request for financial advice, and your response will be shown to the portfolio \
         owner with a clear disclaimer that it is not financial advice — please write accordingly: \
         describe what you observe (concentration, sector skew, notable gainers/losers), and where \
         relevant note general considerations a holder in this situation might want to research further, \
         without telling them what to buy or sell.\n\n\
         Holdings:\n{holdings_summary}\n\n\
         Sector allocation:\n{sector_allocation}\n\
         {xirr_line}\n\
         Please provide: (1) a brief overview of the portfolio's composition, (2) any notable \
         concentration or diversification observations, (3) 2-3 general questions or areas worth the \
         owner's own further research, given what's here. Keep it concise — a few short paragraphs, not \
         an exhaustive report.\n\n\
         FORMAT: write each distinct point as its own line, and prefix every line with exactly one of \
         these tags, whichever best fits that specific point — not the whole response as one tag: \
         [CONCERN] for something worth caution (concentration risk, a large unrealized loss, heavy \
         sector skew), [POSITIVE] for something going well (strong gains, good diversification, \
         healthy returns), [QUESTION] for something worth the owner researching further, [INFO] for \
         plain factual observations that are neither positive nor concerning. Example line: \"[CONCERN] \
         Energy makes up 42% of the portfolio, well above typical single-sector guidance.\" Use these \
         tags for every line of substance — don't leave any point untagged, and don't invent other tags."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_real_shaped_anthropic_response() {
        let sample = r#"{"id": "msg_1", "type": "message", "role": "assistant", "content": [{"type": "text", "text": "Your portfolio shows..."}], "model": "claude-sonnet-5"}"#;
        let parsed: AnthropicResponse = serde_json::from_str(sample).unwrap();
        assert_eq!(parsed.content[0].text.as_deref(), Some("Your portfolio shows..."));
        assert!(parsed.error.is_none());
    }

    #[test]
    fn parses_a_real_shaped_anthropic_error() {
        let sample = r#"{"type": "error", "content": [], "error": {"type": "authentication_error", "message": "invalid x-api-key"}}"#;
        let parsed: AnthropicResponse = serde_json::from_str(sample).unwrap();
        assert_eq!(parsed.error.unwrap().message, "invalid x-api-key");
    }

    #[test]
    fn parses_a_real_shaped_openai_response() {
        let sample = r#"{"id": "chatcmpl-1", "choices": [{"index": 0, "message": {"role": "assistant", "content": "Your portfolio shows..."}, "finish_reason": "stop"}]}"#;
        let parsed: OpenAiResponse = serde_json::from_str(sample).unwrap();
        assert_eq!(parsed.choices[0].message.content, "Your portfolio shows...");
    }

    #[test]
    fn parses_a_real_shaped_gemini_response() {
        let sample = r#"{"candidates": [{"content": {"parts": [{"text": "Your portfolio shows..."}], "role": "model"}, "finishReason": "STOP"}]}"#;
        let parsed: GeminiResponse = serde_json::from_str(sample).unwrap();
        assert_eq!(parsed.candidates[0].content.parts[0].text, "Your portfolio shows...");
    }

    #[test]
    fn prompt_includes_the_not_advice_framing_and_all_provided_data() {
        let prompt = build_portfolio_prompt("My Portfolio", "RELIANCE: 300 shares", "Energy: 40%", Some(12.5));
        assert!(prompt.contains("not a request for financial advice"));
        assert!(prompt.contains("RELIANCE: 300 shares"));
        assert!(prompt.contains("Energy: 40%"));
        assert!(prompt.contains("12.50%"));
    }

    #[test]
    fn prompt_instructs_the_model_to_tag_every_line_for_frontend_color_coding() {
        let prompt = build_portfolio_prompt("My Portfolio", "RELIANCE: 300 shares", "Energy: 40%", None);
        for tag in ["[CONCERN]", "[POSITIVE]", "[QUESTION]", "[INFO]"] {
            assert!(prompt.contains(tag), "prompt should mention the {tag} tag so the model knows to use it");
        }
    }

    #[test]
    fn prompt_omits_xirr_line_cleanly_when_not_available() {
        let prompt = build_portfolio_prompt("My Portfolio", "RELIANCE: 300 shares", "Energy: 40%", None);
        assert!(!prompt.contains("XIRR"));
    }
}
