//! LLM calls during setup — multi-provider support for soul generation.
//!
//! Detects provider from API key prefix and uses the appropriate API format.
//! Supports Anthropic, OpenRouter, OpenAI, and OpenAI-compatible endpoints.

use anyhow::{Context, Result};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const OPENAI_CODEX_RESPONSES_URL: &str = "https://chatgpt.com/backend-api/codex/responses";
const OPENAI_CODEX_ORIGINATOR: &str = "temperpaw";
const OPENAI_CODEX_USER_AGENT: &str = "temperpaw";
const CHATGPT_ACCOUNT_ID_CLAIM: &str = "https://api.openai.com/auth";

/// Detected LLM provider with its configuration.
pub enum LlmProvider {
    Anthropic {
        api_key: String,
        model: String,
    },
    OpenRouter {
        api_key: String,
        model: String,
    },
    OpenAi {
        api_key: String,
        model: String,
    },
    OpenAiCodex {
        access_token: String,
        account_id: String,
        api_url: String,
        model: String,
    },
    OpenAiCompatible {
        provider: String,
        api_key: String,
        api_url: String,
        model: String,
    },
}

impl LlmProvider {
    /// Detect provider from API key prefix.
    pub fn detect(api_key: &str, provider_hint: &str, model: &str) -> Result<Self> {
        if model.trim().is_empty() {
            anyhow::bail!("LLM model is required for setup LLM calls");
        }
        let provider_hint = normalize_provider_hint(provider_hint);
        if api_key.starts_with("sk-ant-") || provider_hint == "anthropic" {
            Ok(LlmProvider::Anthropic {
                api_key: api_key.to_string(),
                model: model.to_string(),
            })
        } else if api_key.starts_with("sk-or-") || provider_hint == "openrouter" {
            Ok(LlmProvider::OpenRouter {
                api_key: api_key.to_string(),
                model: model.to_string(),
            })
        } else if provider_hint == "openai_codex" {
            anyhow::bail!(
                "openai_codex setup calls require ChatGPT/Codex OAuth account metadata and must not fall through to public OpenAI"
            );
        } else if let Some(api_url) = default_openai_compatible_url(&provider_hint) {
            Ok(LlmProvider::OpenAiCompatible {
                provider: provider_hint,
                api_key: api_key.to_string(),
                api_url: api_url.to_string(),
                model: model.to_string(),
            })
        } else {
            Ok(LlmProvider::OpenAi {
                api_key: api_key.to_string(),
                model: model.to_string(),
            })
        }
    }

    pub fn openai_codex(access_token: &str, account_id: Option<&str>, model: &str) -> Result<Self> {
        if model.trim().is_empty() {
            anyhow::bail!("LLM model is required for setup LLM calls");
        }
        if access_token.trim().is_empty() {
            anyhow::bail!("openai_codex requires openai_codex_access_token");
        }
        let account_id = account_id
            .filter(|value| !value.trim().is_empty())
            .map(str::to_string)
            .or_else(|| extract_chatgpt_account_id_from_jwt(access_token))
            .context(
                "openai_codex requires openai_codex_account_id or a ChatGPT OAuth token containing chatgpt_account_id",
            )?;

        Ok(Self::OpenAiCodex {
            access_token: access_token.to_string(),
            account_id,
            api_url: OPENAI_CODEX_RESPONSES_URL.to_string(),
            model: model.to_string(),
        })
    }

    pub fn openai_compatible(
        provider: &str,
        api_key: &str,
        api_url: &str,
        model: &str,
    ) -> Result<Self> {
        if model.trim().is_empty() {
            anyhow::bail!("LLM model is required for setup LLM calls");
        }
        if api_url.trim().is_empty() {
            anyhow::bail!("{provider} requires an OpenAI-compatible API URL");
        }
        Ok(Self::OpenAiCompatible {
            provider: normalize_provider_hint(provider),
            api_key: api_key.to_string(),
            api_url: api_url.to_string(),
            model: model.to_string(),
        })
    }

    /// Make an LLM call and return the text response.
    pub async fn call(&self, system: &str, user_msg: &str, max_tokens: u32) -> Result<String> {
        let client = reqwest::Client::new();

        match self {
            LlmProvider::Anthropic { api_key, model } => {
                let body = serde_json::json!({
                    "model": model,
                    "max_tokens": max_tokens,
                    "system": system,
                    "messages": [{"role": "user", "content": user_msg}]
                });
                let resp = client
                    .post("https://api.anthropic.com/v1/messages")
                    .header("x-api-key", api_key)
                    .header("anthropic-version", "2023-06-01")
                    .header("content-type", "application/json")
                    .json(&body)
                    .send()
                    .await
                    .context("Anthropic API call failed")?;

                let status = resp.status();
                let data: serde_json::Value = resp
                    .json()
                    .await
                    .context("Failed to parse Anthropic response")?;
                if !status.is_success() {
                    let msg = data["error"]["message"].as_str().unwrap_or("Unknown error");
                    anyhow::bail!("Anthropic API error ({}): {}", status, msg);
                }
                data["content"][0]["text"]
                    .as_str()
                    .map(|s| s.to_string())
                    .context("No text in Anthropic response")
            }
            LlmProvider::OpenRouter { api_key, model } => {
                let body = serde_json::json!({
                    "model": model,
                    "max_tokens": max_tokens,
                    "messages": [
                        {"role": "system", "content": system},
                        {"role": "user", "content": user_msg}
                    ]
                });
                let resp = client
                    .post("https://openrouter.ai/api/v1/chat/completions")
                    .header("Authorization", format!("Bearer {api_key}"))
                    .header("content-type", "application/json")
                    .json(&body)
                    .send()
                    .await
                    .context("OpenRouter API call failed")?;

                let status = resp.status();
                let data: serde_json::Value = resp
                    .json()
                    .await
                    .context("Failed to parse OpenRouter response")?;
                if !status.is_success() {
                    let msg = data["error"]["message"].as_str().unwrap_or("Unknown error");
                    anyhow::bail!("OpenRouter API error ({}): {}", status, msg);
                }
                data["choices"][0]["message"]["content"]
                    .as_str()
                    .map(|s| s.to_string())
                    .context("No content in OpenRouter response")
            }
            LlmProvider::OpenAi { api_key, model } => {
                let body = serde_json::json!({
                    "model": model,
                    "max_tokens": max_tokens,
                    "messages": [
                        {"role": "system", "content": system},
                        {"role": "user", "content": user_msg}
                    ]
                });
                let resp = client
                    .post("https://api.openai.com/v1/chat/completions")
                    .header("Authorization", format!("Bearer {api_key}"))
                    .header("content-type", "application/json")
                    .json(&body)
                    .send()
                    .await
                    .context("OpenAI API call failed")?;

                let status = resp.status();
                let data: serde_json::Value = resp
                    .json()
                    .await
                    .context("Failed to parse OpenAI response")?;
                if !status.is_success() {
                    let msg = data["error"]["message"].as_str().unwrap_or("Unknown error");
                    anyhow::bail!("OpenAI API error ({}): {}", status, msg);
                }
                data["choices"][0]["message"]["content"]
                    .as_str()
                    .map(|s| s.to_string())
                    .context("No content in OpenAI response")
            }
            LlmProvider::OpenAiCodex {
                access_token,
                account_id,
                api_url,
                model,
            } => {
                let body = build_openai_codex_responses_body(model, system, user_msg, max_tokens);
                let resp = client
                    .post(api_url)
                    .headers(openai_codex_headers(access_token, account_id))
                    .json(&body)
                    .send()
                    .await
                    .context("OpenAI Codex API call failed")?;

                let status = resp.status();
                let body = resp
                    .text()
                    .await
                    .context("Failed to read OpenAI Codex response")?;
                if !status.is_success() {
                    let msg = response_error_message(&body)
                        .unwrap_or_else(|| "Unknown error".to_string());
                    anyhow::bail!("OpenAI Codex API error ({}): {}", status, msg);
                }
                parse_openai_codex_response_text(&body)
                    .context("No content in OpenAI Codex response")
            }
            LlmProvider::OpenAiCompatible {
                provider,
                api_key,
                api_url,
                model,
            } => {
                let body = serde_json::json!({
                    "model": model,
                    "max_tokens": max_tokens,
                    "messages": [
                        {"role": "system", "content": system},
                        {"role": "user", "content": user_msg}
                    ]
                });
                let mut request = client
                    .post(api_url)
                    .header("content-type", "application/json")
                    .json(&body);
                if !api_key.trim().is_empty() {
                    request = request.header("Authorization", format!("Bearer {api_key}"));
                }
                let resp = request
                    .send()
                    .await
                    .with_context(|| format!("{provider} API call failed"))?;

                let status = resp.status();
                let data: serde_json::Value = resp
                    .json()
                    .await
                    .with_context(|| format!("Failed to parse {provider} response"))?;
                if !status.is_success() {
                    let msg = data["error"]["message"].as_str().unwrap_or("Unknown error");
                    anyhow::bail!("{provider} API error ({}): {}", status, msg);
                }
                data["choices"][0]["message"]["content"]
                    .as_str()
                    .map(|s| s.to_string())
                    .with_context(|| format!("No content in {provider} response"))
            }
        }
    }
}

fn normalize_provider_hint(provider: &str) -> String {
    match provider.trim().to_ascii_lowercase().as_str() {
        "codex" | "openai-codex" => "openai_codex".to_string(),
        "hf" | "hugging_face" | "hugging-face" => "huggingface".to_string(),
        "fireworks_ai" | "fireworks-ai" => "fireworks".to_string(),
        "sakana" | "sakana-fugu" | "fugu" => "sakana_fugu".to_string(),
        "ollama" | "local" | "local-openai" => "local_openai".to_string(),
        "openai-compatible" | "openai_compat" | "openai-compat" | "custom_openai" => {
            "openai_compatible".to_string()
        }
        other => other.to_string(),
    }
}

fn default_openai_compatible_url(provider: &str) -> Option<&'static str> {
    match provider {
        "huggingface" => Some("https://router.huggingface.co/v1/chat/completions"),
        "fireworks" => Some("https://api.fireworks.ai/inference/v1/chat/completions"),
        "local_openai" => Some("http://127.0.0.1:11434/v1/chat/completions"),
        _ => None,
    }
}

fn build_openai_codex_responses_body(
    model: &str,
    system: &str,
    user_msg: &str,
    _max_tokens: u32,
) -> Value {
    json!({
        "model": model,
        "instructions": system,
        "input": [{
            "role": "user",
            "content": user_msg,
        }],
        "stream": true,
        "store": false,
        "reasoning": {
            "effort": "medium",
            "summary": "auto",
        },
    })
}

fn openai_codex_headers(access_token: &str, account_id: &str) -> reqwest::header::HeaderMap {
    use reqwest::header::{
        ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue, USER_AGENT,
    };

    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {access_token}"))
            .expect("OAuth access tokens must be valid HTTP header values"),
    );
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(ACCEPT, HeaderValue::from_static("text/event-stream"));
    headers.insert(
        "chatgpt-account-id",
        HeaderValue::from_str(account_id).expect("ChatGPT account IDs must be header-safe"),
    );
    headers.insert(
        "originator",
        HeaderValue::from_static(OPENAI_CODEX_ORIGINATOR),
    );
    headers.insert(
        USER_AGENT,
        HeaderValue::from_static(OPENAI_CODEX_USER_AGENT),
    );
    headers.insert("openai-beta", HeaderValue::from_static("responses=v1"));
    headers
}

fn parse_openai_codex_response_text(body: &str) -> Option<String> {
    if let Ok(parsed) = serde_json::from_str::<Value>(body)
        && let Some(text) = parse_openai_responses_text(&parsed)
    {
        return Some(text);
    }

    parse_openai_responses_text(&collect_codex_sse_output(body))
}

fn parse_openai_responses_text(parsed: &Value) -> Option<String> {
    let output = parsed.get("output")?.as_array()?;
    for item in output {
        if item.get("type").and_then(Value::as_str) != Some("message") {
            continue;
        }
        let Some(content) = item.get("content").and_then(Value::as_array) else {
            continue;
        };
        let mut combined = String::new();
        for part in content {
            if let Some(text) = part.get("text").and_then(Value::as_str) {
                combined.push_str(text);
            }
        }
        if !combined.trim().is_empty() {
            return Some(combined);
        }
    }
    None
}

fn collect_codex_sse_output(body: &str) -> Value {
    let mut output_items: Vec<Value> = Vec::new();
    let mut streamed_text = String::new();

    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() || line == "[DONE]" || line.starts_with("event:") {
            continue;
        }
        let json_str = line.strip_prefix("data: ").unwrap_or(line);
        let Ok(event) = serde_json::from_str::<Value>(json_str) else {
            continue;
        };
        match event.get("type").and_then(Value::as_str).unwrap_or("") {
            "response.output_item.done" => {
                if let Some(item) = event.get("item") {
                    output_items.push(item.clone());
                }
            }
            "response.output_text.delta" => {
                if let Some(delta) = event.get("delta").and_then(Value::as_str) {
                    streamed_text.push_str(delta);
                } else if let Some(text) = event.get("text").and_then(Value::as_str) {
                    streamed_text.push_str(text);
                }
            }
            "response.output_text.done" => {
                if streamed_text.is_empty()
                    && let Some(text) = event.get("text").and_then(Value::as_str)
                {
                    streamed_text.push_str(text);
                }
            }
            "response.completed" => {
                if let Some(out) = event
                    .get("response")
                    .and_then(|response| response.get("output"))
                    .and_then(Value::as_array)
                    && !out.is_empty()
                {
                    output_items = out.clone();
                }
            }
            _ => {}
        }
    }

    if output_items.is_empty() {
        let text = streamed_text.trim();
        if !text.is_empty() {
            output_items.push(json!({
                "type": "message",
                "content": [{ "type": "output_text", "text": text }],
            }));
        }
    }

    json!({ "output": output_items })
}

fn response_error_message(body: &str) -> Option<String> {
    if let Ok(parsed) = serde_json::from_str::<Value>(body) {
        if let Some(message) = parsed
            .get("error")
            .and_then(|error| error.get("message"))
            .and_then(Value::as_str)
            .or_else(|| parsed.get("message").and_then(Value::as_str))
        {
            return Some(message.to_string());
        }
    }

    response_body_snippet(body)
}

fn response_body_snippet(body: &str) -> Option<String> {
    const MAX_ERROR_BODY_SNIPPET_CHARS: usize = 512;
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut snippet: String = trimmed.chars().take(MAX_ERROR_BODY_SNIPPET_CHARS).collect();
    if trimmed.chars().count() > MAX_ERROR_BODY_SNIPPET_CHARS {
        snippet.push_str("…");
    }
    Some(snippet)
}

fn extract_chatgpt_account_id_from_jwt(token: &str) -> Option<String> {
    let payload = token.split('.').nth(1)?;
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()
        .or_else(|| {
            base64::engine::general_purpose::URL_SAFE
                .decode(payload)
                .ok()
        })?;
    let parsed: Value = serde_json::from_slice(&decoded).ok()?;
    parsed
        .get(CHATGPT_ACCOUNT_ID_CLAIM)?
        .get("chatgpt_account_id")?
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod provider_tests {
    use super::*;

    #[test]
    fn codex_provider_hint_cannot_fall_through_to_public_openai() {
        match LlmProvider::detect("chatgpt-oauth-token", "openai_codex", "gpt-5.5") {
            Ok(_) => panic!("Codex OAuth tokens must not be treated as public OpenAI keys"),
            Err(err) => assert!(err.to_string().contains("openai_codex")),
        }
    }

    #[test]
    fn aliases_normalize_to_codex_provider_hint() {
        assert_eq!(normalize_provider_hint("codex"), "openai_codex");
        assert_eq!(normalize_provider_hint("openai-codex"), "openai_codex");
    }

    #[test]
    fn codex_setup_request_matches_subscription_responses_contract() {
        let body = build_openai_codex_responses_body("gpt-5.5", "system", "hello", 1024);

        assert_eq!(body["model"], "gpt-5.5");
        assert_eq!(body["instructions"], "system");
        assert_eq!(body["stream"], true);
        assert_eq!(body["store"], false);
        assert_eq!(body["reasoning"]["effort"], "medium");
        assert_eq!(body["reasoning"]["summary"], "auto");
        assert!(
            body.get("max_output_tokens").is_none(),
            "ChatGPT/Codex subscription responses reject the public Responses max_output_tokens field"
        );
    }

    #[test]
    fn codex_error_message_falls_back_to_body_snippet() {
        assert_eq!(
            response_error_message("unsupported field: max_output_tokens").as_deref(),
            Some("unsupported field: max_output_tokens")
        );

        let msg = response_error_message(r#"{"detail":"bad request"}"#)
            .expect("JSON error without error.message should still be surfaced");
        assert!(msg.contains("bad request"));
    }
}

/// Interview data collected from the user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInterview {
    pub name: String,
    pub about_you: String,
    pub ideal_paw: String,
    pub followup_answers: Vec<(String, String)>,
}

/// Generated soul artifacts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedSoul {
    pub soul_md: String,
    pub style_md: String,
    pub user_md: String,
    pub summary: String,
}

const FOLLOWUP_SYSTEM: &str = "\
You're getting to know someone who's setting up an AI teammate. They just told you about themselves and what they want. Ask 1-2 follow-up questions that:

- Are specific to what they said, not generic
- Reveal something they didn't think to mention — a value, a tension, a preference
- Feel fun and genuine, not like a corporate questionnaire
- Are short (one sentence each)

Output ONLY the questions, one per line, prefixed with Q:";

const SOUL_SYSTEM: &str = "\
You are generating a personalized agent soul for Temper Paw.

Temper Paw's chief of staff agent (\"Paw\") manages software projects and creative work through specialized project leads. Paw doesn't write code directly — it sets direction, crafts the right lead for each project, and ensures things land. But Paw adapts deeply to the human it works with.

The human below has told you about themselves and what kind of Paw they want. Generate content that is deeply personalized — not a template with blanks filled in, but a soul that reads like it was written by someone who knows this person.

Output exactly four sections separated by these markers on their own lines:

---SOUL---
A SOUL.md following this structure:
## Who I Am
## Worldview (2-4 beliefs specific to this human's domain and values)
## Opinions (on engineering, planning, communication, scope — adapted to this human)
## How I Think
## Tensions I Hold
## Boundaries

---STYLE---
A STYLE.md covering:
## Voice (register, density, confidence — matched to what the human wants)
## Sentence Structure
## Vocabulary (domain-specific terms, words to avoid)
## What Right Sounds Like (2-3 example lines in this Paw's voice)
## What Wrong Sounds Like (1-2 anti-examples)

---USER---
A USER.md profile of the human:
## [Their Name]
### Who They Are
### What They Value
### How They Work
### What They Need From Paw

---SUMMARY---
2-3 sentences describing what this Paw will be like, written directly to the human in second person. This is shown as a preview before they commit. Make it vivid and specific, not generic.";

/// Generate 1-2 follow-up questions based on the user's initial answers.
pub async fn generate_followup_questions(
    provider: &LlmProvider,
    name: &str,
    about_you: &str,
    ideal_paw: &str,
) -> Result<Vec<String>> {
    let user_msg = format!(
        "Name: {name}\n\nAbout themselves:\n{about_you}\n\nWhat kind of Paw they want:\n{ideal_paw}"
    );

    let response = provider.call(FOLLOWUP_SYSTEM, &user_msg, 512).await?;

    let questions: Vec<String> = response
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if let Some(q) = trimmed.strip_prefix("Q:") {
                let q = q.trim();
                if !q.is_empty() {
                    return Some(q.to_string());
                }
            }
            None
        })
        .take(2)
        .collect();

    if questions.is_empty() {
        anyhow::bail!("No follow-up questions generated");
    }
    Ok(questions)
}

/// Generate the full personalized soul from all interview data.
pub async fn generate_personalized_soul(
    provider: &LlmProvider,
    interview: &UserInterview,
) -> Result<GeneratedSoul> {
    let mut user_msg = format!(
        "Name: {}\n\nAbout themselves:\n{}\n\nWhat kind of Paw they want:\n{}",
        interview.name, interview.about_you, interview.ideal_paw
    );

    if !interview.followup_answers.is_empty() {
        user_msg.push_str("\n\nFollow-up answers:");
        for (question, answer) in &interview.followup_answers {
            user_msg.push_str(&format!("\n\nQ: {question}\nA: {answer}"));
        }
    }

    let response = provider.call(SOUL_SYSTEM, &user_msg, 4096).await?;
    parse_generated_soul(&response)
}

/// Generate a refined soul based on user feedback on a previous generation.
pub async fn refine_soul(
    provider: &LlmProvider,
    interview: &UserInterview,
    previous_summary: &str,
    feedback: &str,
) -> Result<GeneratedSoul> {
    let mut user_msg = format!(
        "Name: {}\n\nAbout themselves:\n{}\n\nWhat kind of Paw they want:\n{}",
        interview.name, interview.about_you, interview.ideal_paw
    );

    for (question, answer) in &interview.followup_answers {
        user_msg.push_str(&format!("\n\nQ: {question}\nA: {answer}"));
    }

    user_msg.push_str(&format!(
        "\n\n---\nPrevious attempt summary: {previous_summary}\n\nUser feedback: {feedback}\n\nRegenerate the soul incorporating this feedback."
    ));

    let response = provider.call(SOUL_SYSTEM, &user_msg, 4096).await?;
    parse_generated_soul(&response)
}

fn parse_generated_soul(response: &str) -> Result<GeneratedSoul> {
    let extract = |start: &str, end: Option<&str>| -> String {
        let Some(start_idx) = response.find(start) else {
            return String::new();
        };
        let content_start = start_idx + start.len();
        let content_end = end
            .and_then(|e| response[content_start..].find(e).map(|i| content_start + i))
            .unwrap_or(response.len());
        response[content_start..content_end].trim().to_string()
    };

    let soul_md = extract("---SOUL---", Some("---STYLE---"));
    let style_md = extract("---STYLE---", Some("---USER---"));
    let user_md = extract("---USER---", Some("---SUMMARY---"));
    let summary = extract("---SUMMARY---", None);

    if soul_md.is_empty() || summary.is_empty() {
        anyhow::bail!("Failed to parse generated soul — missing sections");
    }

    Ok(GeneratedSoul {
        soul_md,
        style_md,
        user_md,
        summary,
    })
}
