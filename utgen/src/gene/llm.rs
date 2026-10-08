use crate::LlmConfig;
use async_openai::types::{
    ChatCompletionRequestMessage, ChatCompletionRequestSystemMessageArgs,
    ChatCompletionRequestUserMessageArgs, CreateChatCompletionRequest,
    CreateChatCompletionRequestArgs, CreateChatCompletionResponse,
};
use serde::Serialize;
use std::{
    fs, io,
    num::NonZeroU64,
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::time::{sleep, timeout};

const MAX_ATTEMPTS: u32 = 3;

#[derive(Default, Serialize)]
struct RequestStats {
    attempts: u64,
    failed_attempts: u64,
    responses_without_usage: u64,
    reported_completion_tokens: u64,
    reported_prompt_tokens: u64,
    budget_exhausted: bool,
}

struct RequestError {
    message: String,
    retryable: bool,
}

impl From<reqwest::Error> for RequestError {
    fn from(error: reqwest::Error) -> Self {
        Self {
            retryable: error.is_connect()
                || error.is_timeout()
                || error.is_body()
                || error.is_request(),
            message: error.to_string(),
        }
    }
}

#[derive(Clone)]
pub struct LLM {
    config: LlmConfig,
    client: reqwest::Client,
    request_timeout: Duration,
    max_requests: Option<NonZeroU64>,
    stats: Arc<Mutex<RequestStats>>,
}

impl LLM {
    pub fn new(config: LlmConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
            request_timeout: Duration::from_secs(180),
            max_requests: None,
            stats: Arc::new(Mutex::new(RequestStats::default())),
        }
    }

    pub fn with_request_timeout(mut self, duration: Duration) -> Self {
        self.request_timeout = duration;
        self
    }

    pub fn request_count(&self) -> u64 {
        self.stats.lock().unwrap().attempts
    }

    pub fn with_max_requests(mut self, max_requests: Option<NonZeroU64>) -> Self {
        self.max_requests = max_requests;
        self
    }

    /// Persist reported usage, including an explicit incomplete-usage marker.
    pub fn write_request_summary(&self, path: &Path, invocation: serde_json::Value) -> io::Result<()> {
        let stats = self.stats.lock().unwrap();
        let mut summary = serde_json::to_value(&*stats)?;
        summary["usage_complete"] =
            (stats.failed_attempts == 0 && stats.responses_without_usage == 0).into();
        summary["model"] = self.config.model.clone().into();
        summary["request_timeout_seconds"] = self.request_timeout.as_secs_f64().into();
        summary["max_requests"] = serde_json::to_value(self.max_requests)?;
        summary["invocation"] = invocation;
        fs::create_dir_all(path.parent().unwrap())?;
        fs::write(path, serde_json::to_vec_pretty(&summary)?)
    }

    async fn send_once(
        &self,
        request: &CreateChatCompletionRequest,
    ) -> Result<CreateChatCompletionResponse, RequestError> {
        let response = self
            .client
            .post(format!(
                "{}/chat/completions",
                self.config.base.trim_end_matches('/')
            ))
            .bearer_auth(&self.config.key)
            .json(request)
            .send()
            .await?;
        let status = response.status();
        let body = response.bytes().await?;
        if !status.is_success() {
            let detail: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
            let error = &detail["error"];
            let quota_exhausted =
                error["code"] == "insufficient_quota" || error["type"] == "insufficient_quota";
            return Err(RequestError {
                message: format!(
                    "Model HTTP {status}: {}",
                    error["message"].as_str().unwrap_or("request failed")
                ),
                retryable: (status.as_u16() == 429 && !quota_exhausted) || status.is_server_error(),
            });
        }
        serde_json::from_slice(&body).map_err(|error| RequestError {
            message: format!("Invalid model response JSON: {error}"),
            retryable: false,
        })
    }

    pub async fn fetch_answer(
        &self,
        system_pt: Option<&str>,
        user_pt: &str,
        n: u8,
        stream: bool,
    ) -> io::Result<(Vec<String>, u32, u32)> {
        if n != 1 || stream {
            return Err(io::Error::other(
                "PALM supports one non-streaming answer per request",
            ));
        }
        let mut messages = Vec::new();
        if let Some(system) = system_pt {
            messages.push(ChatCompletionRequestMessage::System(
                ChatCompletionRequestSystemMessageArgs::default()
                    .content(system)
                    .build()
                    .map_err(io::Error::other)?,
            ));
        }
        messages.push(ChatCompletionRequestMessage::User(
            ChatCompletionRequestUserMessageArgs::default()
                .content(user_pt)
                .build()
                .map_err(io::Error::other)?,
        ));
        let request = CreateChatCompletionRequestArgs::default()
            .model(&self.config.model)
            .max_tokens(10000_u32)
            .n(1)
            .stream(false)
            .messages(messages)
            .build()
            .map_err(io::Error::other)?;
        for attempt in 1..=MAX_ATTEMPTS {
            {
                let mut stats = self.stats.lock().unwrap();
                if let Some(max) = self.max_requests {
                    if stats.attempts >= max.get() {
                        stats.budget_exhausted = true;
                        return Err(io::Error::other(format!(
                            "Model request budget exhausted (--max-requests {max})"
                        )));
                    }
                }
                stats.attempts += 1;
            }
            let response = match timeout(self.request_timeout, self.send_once(&request)).await {
                Ok(result) => result,
                Err(_) => Err(RequestError {
                    message: format!(
                        "Model request timed out after {}s",
                        self.request_timeout.as_secs_f64()
                    ),
                    retryable: true,
                }),
            };
            match response {
                Ok(response) => {
                    let mut stats = self.stats.lock().unwrap();
                    let (completion, prompt) = match response.usage {
                        Some(usage) => {
                            stats.reported_completion_tokens += u64::from(usage.completion_tokens);
                            stats.reported_prompt_tokens += u64::from(usage.prompt_tokens);
                            (usage.completion_tokens, usage.prompt_tokens)
                        }
                        None => {
                            stats.responses_without_usage += 1;
                            log::warn!(
                                "Model response has no usage; reported token totals are incomplete"
                            );
                            (0, 0)
                        }
                    };
                    if response.choices.len() != 1 {
                        return Err(io::Error::other(
                            "Model response must contain exactly one choice",
                        ));
                    }
                    let answer = response
                        .choices
                        .into_iter()
                        .next()
                        .unwrap()
                        .message
                        .content
                        .filter(|content| !content.trim().is_empty())
                        .ok_or_else(|| {
                            io::Error::other("Model response contains no answer text")
                        })?;
                    return Ok((vec![answer], completion, prompt));
                }
                Err(error) => {
                    self.stats.lock().unwrap().failed_attempts += 1;
                    if !error.retryable || attempt == MAX_ATTEMPTS {
                        return Err(io::Error::other(format!(
                            "{} (attempt {attempt}/{MAX_ATTEMPTS})",
                            error.message
                        )));
                    }
                    log::warn!("{}; retrying ({attempt}/{MAX_ATTEMPTS})", error.message);
                    sleep(Duration::from_secs(u64::from(attempt))).await;
                }
            }
        }
        unreachable!("the last attempt returns success or failure")
    }

    pub async fn get_answer(
        &self,
        prompt: &str,
        n: u8,
        stream: bool,
    ) -> io::Result<(Vec<String>, u32, u32)> {
        self.fetch_answer(None, prompt, n, stream).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        time::{Duration, timeout},
    };

    fn completion() -> serde_json::Value {
        serde_json::json!({
            "id": "local-response", "object": "chat.completion", "created": 0, "model": "local-model",
            "choices": [{"index": 0, "message": {"role": "assistant", "content": "local answer"}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 2, "completion_tokens": 3, "total_tokens": 5}
        })
    }

    fn reply(status: u16) -> (u16, String, Duration) {
        let body = if status == 200 {
            completion()
        } else {
            serde_json::json!({"error": {"message": "fixture error"}})
        };
        (status, body.to_string(), Duration::ZERO)
    }

    // Real HTTP, with counts and delayed bodies to verify the entire attempt deadline.
    async fn mock_replies(
        replies: Vec<(u16, String, Duration)>,
    ) -> (LLM, tokio::task::JoinHandle<Vec<serde_json::Value>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            timeout(Duration::from_secs(15), async move {
                let mut requests = Vec::new();
                for (status, body, delay) in replies {
                    let (mut socket, _) = listener.accept().await.unwrap();
                    let mut data = Vec::new();
                    let (headers_end, length) = loop {
                        let mut buffer = [0; 2048];
                        let count = socket.read(&mut buffer).await.unwrap();
                        assert!(count > 0, "request ended before headers");
                        data.extend_from_slice(&buffer[..count]);
                        if let Some(end) = data.windows(4).position(|s| s == b"\r\n\r\n") {
                            let headers = String::from_utf8_lossy(&data[..end]).to_lowercase();
                            assert!(headers.starts_with("post /v1/chat/completions http/1.1"));
                            assert!(headers.contains("authorization: bearer local-test-key"));
                            let length = headers.lines().find_map(|line| {
                                line.strip_prefix("content-length:").map(|n| n.trim().parse::<usize>().unwrap())
                            }).unwrap();
                            break (end + 4, length);
                        }
                    };
                    while data.len() < headers_end + length {
                        let mut buffer = [0; 2048];
                        let count = socket.read(&mut buffer).await.unwrap();
                        assert!(count > 0, "request ended before body");
                        data.extend_from_slice(&buffer[..count]);
                    }
                    requests.push(serde_json::from_slice(&data[headers_end..headers_end + length]).unwrap());
                    if status == 0 { continue; } // Deliberately drop the response connection.
                    let header = format!("HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                    socket.write_all(header.as_bytes()).await.unwrap();
                    sleep(delay).await;
                    // A timeout deliberately disconnects before its delayed body arrives.
                    let result = socket.write_all(body.as_bytes()).await;
                    if delay.is_zero() { result.unwrap(); }
                }
                requests
            }).await.expect("local mock server timed out")
        });
        let mut llm = LLM::new(LlmConfig {
            base: format!("http://{address}/v1"),
            key: "local-test-key".into(),
            model: "local-model".into(),
        });
        // Only fixtures bypass proxies; normal clients retain proxy discovery.
        llm.client = reqwest::Client::builder().no_proxy().build().unwrap();
        (llm, server)
    }

    async fn mock_model() -> (LLM, tokio::task::JoinHandle<Vec<serde_json::Value>>) {
        mock_replies(vec![reply(200)]).await
    }

    fn check_request(request: &serde_json::Value) {
        assert_eq!(request["model"], "local-model");
        assert_eq!(request["max_tokens"], 10000);
        assert!(request.get("temperature").is_none());
        assert!(request.get("top_p").is_none());
        assert_eq!(request["n"], 1);
        assert_eq!(request["stream"], false);
    }

    #[test]
    fn mock_requests_bypass_environment_proxies() {
        // Set proxy variables only in child processes: changing the current
        // process environment would race with parallel Rust tests.
        for name in [
            "gene::llm::tests::generation_uses_runtime_config_and_model_sampling_defaults",
            "gene::llm::tests::repair_uses_runtime_config_and_model_sampling_defaults",
        ] {
            let mut command = std::process::Command::new(std::env::current_exe().unwrap());
            command.args([name, "--exact"]);
            for variable in [
                "HTTP_PROXY",
                "HTTPS_PROXY",
                "ALL_PROXY",
                "http_proxy",
                "https_proxy",
                "all_proxy",
            ] {
                command.env(variable, "http://127.0.0.1:9");
            }
            command.env("NO_PROXY", "").env("no_proxy", "");
            let output = command.output().unwrap();
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(
                output.status.success() && stdout.contains("1 passed"),
                "{name} failed with proxy environment variables:\n{stdout}\n{}",
                String::from_utf8_lossy(&output.stderr),
            );
        }
    }

    #[tokio::test]
    async fn generation_uses_runtime_config_and_model_sampling_defaults() {
        let (llm, server) = mock_model().await;
        let (answers, completion, prompt) = timeout(
            Duration::from_secs(10),
            llm.fetch_answer(Some("system instruction"), "generate a test", 1, false),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(answers, vec!["local answer"]);
        assert_eq!((completion, prompt), (3, 2));
        let request = server.await.unwrap().remove(0);
        check_request(&request);
        assert_eq!(request["messages"][0]["role"], "system");
        assert_eq!(request["messages"][1]["content"], "generate a test");
    }

    #[tokio::test]
    async fn repair_uses_runtime_config_and_model_sampling_defaults() {
        let (llm, server) = mock_model().await;
        let (answers, completion, prompt) = timeout(
            Duration::from_secs(10),
            llm.get_answer("repair a test", 1, false),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(answers, vec!["local answer"]);
        assert_eq!((completion, prompt), (3, 2));
        let request = server.await.unwrap().remove(0);
        check_request(&request);
        assert_eq!(request["messages"][0]["role"], "user");
        assert_eq!(request["messages"][0]["content"], "repair a test");
    }

    #[tokio::test]
    async fn missing_usage_keeps_the_answer_and_marks_the_report_incomplete() {
        let mut body = completion();
        body.as_object_mut().unwrap().remove("usage");
        let (llm, server) = mock_replies(vec![(200, body.to_string(), Duration::ZERO)]).await;
        let answer = llm.fetch_answer(None, "test", 1, false).await.unwrap();
        assert_eq!(answer, (vec!["local answer".into()], 0, 0));
        assert_eq!(server.await.unwrap().len(), 1);
        let path =
            std::env::temp_dir().join(format!("palm-request-usage-{}.json", std::process::id()));
        llm.write_request_summary(&path, serde_json::json!({})).unwrap();
        let summary: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        fs::remove_file(path).unwrap();
        assert_eq!(summary["responses_without_usage"], 1);
        assert_eq!(summary["usage_complete"], false);
    }

    #[tokio::test]
    async fn rejects_empty_or_malformed_answers_without_retrying() {
        let mut bodies = Vec::new();
        let mut empty = completion();
        empty["choices"] = serde_json::json!([]);
        bodies.push(empty.to_string());
        for content in [serde_json::Value::Null, serde_json::json!(" \n ")] {
            let mut body = completion();
            body["choices"][0]["message"]["content"] = content;
            bodies.push(body.to_string());
        }
        bodies.push("{invalid JSON".into());
        for body in bodies {
            let (llm, server) = mock_replies(vec![(200, body, Duration::ZERO)]).await;
            assert!(llm.get_answer("test", 1, false).await.is_err());
            assert_eq!(server.await.unwrap().len(), 1);
            assert_eq!(llm.stats.lock().unwrap().attempts, 1);
        }
    }

    #[tokio::test]
    async fn permanent_http_errors_do_not_retry() {
        for status in [400, 401, 403, 404, 422] {
            let (llm, server) = mock_replies(vec![reply(status)]).await;
            let error = llm.get_answer("test", 1, false).await.unwrap_err();
            assert!(error.to_string().contains(&status.to_string()));
            assert_eq!(server.await.unwrap().len(), 1);
            assert_eq!(llm.stats.lock().unwrap().attempts, 1);
        }
        let body =
            serde_json::json!({"error": {"message": "no quota", "code": "insufficient_quota"}});
        let (llm, server) = mock_replies(vec![(429, body.to_string(), Duration::ZERO)]).await;
        assert!(llm.get_answer("test", 1, false).await.is_err());
        assert_eq!(server.await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn transient_errors_retry_and_recover_within_three_attempts() {
        let (llm, server) = mock_replies(vec![reply(429), reply(503), reply(200)]).await;
        assert_eq!(
            llm.get_answer("test", 1, false).await.unwrap().0,
            vec!["local answer"]
        );
        assert_eq!(server.await.unwrap().len(), 3);
        let stats = llm.stats.lock().unwrap();
        assert_eq!((stats.attempts, stats.failed_attempts), (3, 2));
    }

    #[tokio::test]
    async fn a_dropped_connection_is_retried() {
        let (llm, server) = mock_replies(vec![reply(0), reply(200)]).await;
        assert!(llm.get_answer("test", 1, false).await.is_ok());
        assert_eq!(server.await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn transient_errors_stop_after_three_attempts() {
        for status in [429, 500] {
            let (llm, server) =
                mock_replies(vec![reply(status), reply(status), reply(status)]).await;
            assert!(
                llm.get_answer("test", 1, false)
                    .await
                    .unwrap_err()
                    .to_string()
                    .contains("attempt 3/3")
            );
            assert_eq!(server.await.unwrap().len(), 3);
            assert_eq!(llm.stats.lock().unwrap().attempts, 3);
        }
    }

    #[tokio::test]
    async fn request_budget_is_shared_across_concurrent_workers_and_retries() {
        let (llm, server) = mock_replies(vec![reply(503), reply(200), reply(200)]).await;
        let llm = llm.with_max_requests(NonZeroU64::new(3));
        let mut workers = tokio::task::JoinSet::new();
        for _ in 0..9 {
            let llm = llm.clone();
            workers.spawn(async move { llm.get_answer("test", 1, false).await });
        }
        let mut successes = 0;
        while let Some(result) = workers.join_next().await {
            match result.unwrap() {
                Ok(_) => successes += 1,
                Err(error) => assert!(error.to_string().contains("budget exhausted")),
            }
        }
        assert_eq!(successes, 2);
        assert_eq!(server.await.unwrap().len(), 3);
        let stats = llm.stats.lock().unwrap();
        assert_eq!((stats.attempts, stats.failed_attempts), (3, 1));
        assert!(stats.budget_exhausted);
    }

    #[tokio::test]
    async fn request_budget_can_stop_a_single_workers_retries() {
        let (llm, server) = mock_replies(vec![reply(503), reply(503)]).await;
        let llm = llm.with_max_requests(NonZeroU64::new(2));
        let error = llm.get_answer("test", 1, false).await.unwrap_err();
        assert!(error.to_string().contains("budget exhausted"));
        assert_eq!(server.await.unwrap().len(), 2);
        let stats = llm.stats.lock().unwrap();
        assert_eq!((stats.attempts, stats.failed_attempts), (2, 2));
        assert!(stats.budget_exhausted);
    }

    #[tokio::test]
    async fn completing_on_the_last_allowed_attempt_is_successful() {
        let (llm, server) = mock_replies(vec![reply(503), reply(200)]).await;
        let llm = llm.with_max_requests(NonZeroU64::new(2));
        assert!(llm.get_answer("test", 1, false).await.is_ok());
        assert_eq!(server.await.unwrap().len(), 2);
        assert!(!llm.stats.lock().unwrap().budget_exhausted);
    }

    #[tokio::test]
    async fn attempt_timeout_includes_reading_the_body_and_stops_after_three() {
        let delayed = (200, completion().to_string(), Duration::from_millis(150));
        let (llm, server) = mock_replies(vec![delayed.clone(), delayed.clone(), delayed]).await;
        let llm = llm.with_request_timeout(Duration::from_millis(50));
        let error = llm.get_answer("test", 1, false).await.unwrap_err();
        assert!(error.to_string().contains("timed out"));
        assert_eq!(server.await.unwrap().len(), 3);
        assert_eq!(llm.stats.lock().unwrap().failed_attempts, 3);
    }

    #[tokio::test]
    #[ignore = "requires explicitly configured model credentials and makes a real API request"]
    async fn test_llm() {
        let llm = LLM::new(LlmConfig::load(None).unwrap());
        let sys_pt = "Your name is John";
        let user_pt = "Hello, what is your name?";
        let answers = llm
            .fetch_answer(Some(sys_pt), user_pt, 1, false)
            .await
            .unwrap()
            .0;
        assert_eq!(answers.len(), 1);
        // The production workflow currently requests one non-streaming answer.
    }
}
