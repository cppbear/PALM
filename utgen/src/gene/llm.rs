use crate::LlmConfig;
use async_openai::{
    Client,
    config::OpenAIConfig,
    types::{
        ChatCompletionRequestMessage, ChatCompletionRequestSystemMessageArgs,
        ChatCompletionRequestUserMessageArgs, CreateChatCompletionRequestArgs,
    },
};
use futures::StreamExt;

#[derive(Clone)]
pub struct LLM {
    config: LlmConfig,
    client: Client<OpenAIConfig>,
}

impl LLM {
    pub fn new(config: LlmConfig) -> Self {
        let api_config = OpenAIConfig::new()
            .with_api_base(&config.base)
            .with_api_key(&config.key);
        Self {
            config,
            client: Client::with_config(api_config),
        }
    }

    pub async fn fetch_answer(
        &self,
        system_pt: Option<&str>,
        user_pt: &str,
        n: u8,
        stream: bool,
    ) -> Result<(Vec<String>, u32, u32), Box<dyn std::error::Error>> {
        let client = &self.client;
        let system_msg = if system_pt.is_none() {
            None
        } else {
            Some(
                ChatCompletionRequestSystemMessageArgs::default()
                    .content(system_pt.unwrap())
                    .build()?,
            )
        };
        let user_msg = ChatCompletionRequestUserMessageArgs::default()
            .content(user_pt)
            .build()?;
        let messages = if system_msg.is_none() {
            vec![ChatCompletionRequestMessage::User(user_msg)]
        } else {
            vec![
                ChatCompletionRequestMessage::System(system_msg.unwrap()),
                ChatCompletionRequestMessage::User(user_msg),
            ]
        };
        let request = CreateChatCompletionRequestArgs::default()
            .model(&self.config.model)
            .max_tokens(10000_u32)
            .temperature(1.0)
            .top_p(0_f32)
            .n(1)
            .stream(false)
            .messages(messages)
            .build()?;
        let mut result;
        let mut completion_tokens = 0;
        let mut prompt_tokens = 0;
        if !stream {
            let response = client.chat().create(request).await?;
            result = response
                .choices
                .into_iter()
                .filter_map(|c| c.message.content)
                .collect();
            let usage = response.usage.unwrap();
            completion_tokens += usage.completion_tokens;
            prompt_tokens += usage.prompt_tokens;
        } else {
            result = vec!["".to_string(); n as usize];
            let mut stream = client.chat().create_stream(request).await?;
            while let Some(response) = stream.next().await {
                match response {
                    Ok(chunk) => {
                        for choice in chunk.choices.into_iter() {
                            if let Some(content) = choice.delta.content {
                                result[choice.index as usize] += &content;
                            }
                        }
                        let usage = chunk.usage.unwrap();
                        completion_tokens += usage.completion_tokens;
                        prompt_tokens += usage.prompt_tokens;
                    }
                    Err(e) => return Err(Box::new(e)),
                }
            }
        }
        Ok((result, completion_tokens, prompt_tokens))
    }

    pub async fn get_answer(
        &self,
        prompt: &str,
        n: u8,
        stream: bool,
    ) -> Result<(Vec<String>, u32, u32), Box<dyn std::error::Error + Send>> {
        let client = &self.client;
        // let msg = ChatCompletionRequestUserMessageArgs::default()
        //     .content(prompt)
        //     .build()?;
        let msg = ChatCompletionRequestUserMessageArgs::default()
            .content(prompt)
            .build()
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?;
        let mut completion_tokens = 0;
        let mut prompt_tokens = 0;
        let request = CreateChatCompletionRequestArgs::default()
            .model(&self.config.model)
            .max_tokens(10000_u32)
            .temperature(1.0)
            .top_p(0_f32)
            .n(1)
            .stream(false)
            .messages(vec![ChatCompletionRequestMessage::User(msg)])
            .build()
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?;
        let mut result;
        if !stream {
            let response = client
                .chat()
                .create(request)
                .await
                .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?;
            result = response
                .choices
                .into_iter()
                .filter_map(|c| c.message.content)
                .collect();
            let usage = response.usage.unwrap();
            completion_tokens += usage.completion_tokens;
            prompt_tokens += usage.prompt_tokens;
        } else {
            result = vec!["".to_string(); n as usize];
            let mut stream = client
                .chat()
                .create_stream(request)
                .await
                .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?;
            while let Some(response) = stream.next().await {
                match response {
                    Ok(chunk) => {
                        for choice in chunk.choices.into_iter() {
                            if let Some(content) = choice.delta.content {
                                result[choice.index as usize] += &content;
                            }
                        }
                        let usage = chunk.usage.unwrap();
                        completion_tokens += usage.completion_tokens;
                        prompt_tokens += usage.prompt_tokens;
                    }
                    Err(e) => return Err(Box::new(e)),
                }
            }
        }
        Ok((result, completion_tokens, prompt_tokens))
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

    // Exercise the actual HTTP client and request shape without a model service.
    async fn mock_model() -> (LLM, tokio::task::JoinHandle<serde_json::Value>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            timeout(Duration::from_secs(10), async move {
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
                let request = serde_json::from_slice(&data[headers_end..headers_end + length]).unwrap();
                let body = serde_json::json!({
                    "id": "local-response", "object": "chat.completion", "created": 0, "model": "local-model",
                    "choices": [{"index": 0, "message": {"role": "assistant", "content": "local answer"}, "finish_reason": "stop"}],
                    "usage": {"prompt_tokens": 2, "completion_tokens": 3, "total_tokens": 5}
                }).to_string();
                let response = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
                socket.write_all(response.as_bytes()).await.unwrap();
                request
            }).await.expect("local mock server timed out")
        });
        let mut llm = LLM::new(LlmConfig {
            base: format!("http://{address}/v1"),
            key: "local-test-key".into(),
            model: "local-model".into(),
        });
        // Only the local fixture bypasses proxies. LLM::new keeps the SDK's
        // default proxy behavior for production and the opt-in live test.
        llm.client = llm
            .client
            .with_http_client(reqwest::Client::builder().no_proxy().build().unwrap());
        (llm, server)
    }

    fn check_request(request: &serde_json::Value) {
        assert_eq!(request["model"], "local-model");
        assert_eq!(request["max_tokens"], 10000);
        assert_eq!(request["temperature"], 1.0);
        assert_eq!(request["top_p"], 0.0);
        assert_eq!(request["n"], 1);
        assert_eq!(request["stream"], false);
    }

    #[test]
    fn mock_requests_bypass_environment_proxies() {
        // Set proxy variables only in child processes: changing the current
        // process environment would race with parallel Rust tests.
        for name in [
            "gene::llm::tests::generation_uses_runtime_config_and_preserves_request_parameters",
            "gene::llm::tests::repair_uses_runtime_config_and_preserves_request_parameters",
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
    async fn generation_uses_runtime_config_and_preserves_request_parameters() {
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
        let request = server.await.unwrap();
        check_request(&request);
        assert_eq!(request["messages"][0]["role"], "system");
        assert_eq!(request["messages"][1]["content"], "generate a test");
    }

    #[tokio::test]
    async fn repair_uses_runtime_config_and_preserves_request_parameters() {
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
        let request = server.await.unwrap();
        check_request(&request);
        assert_eq!(request["messages"][0]["role"], "user");
        assert_eq!(request["messages"][0]["content"], "repair a test");
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
