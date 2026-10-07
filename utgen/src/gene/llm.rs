use async_openai::{
    config::OpenAIConfig,
    types::{
        ChatCompletionRequestMessage, ChatCompletionRequestSystemMessageArgs,
         ChatCompletionRequestUserMessageArgs, CreateChatCompletionRequestArgs,
    },
    Client,
};
use futures::StreamExt;
use serde::Deserialize;

#[derive(Deserialize, Debug, Clone)]
struct LLMConfig {
    base: String,
    key: String,
    model: String,
}

pub struct LLM {
    config: LLMConfig,
}

impl LLM {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let api_config = include_str!("../../res/api.json");
        let config: LLMConfig = serde_json::from_str(api_config)?;
        Ok(Self { config })
    }

    pub async fn fetch_answer(
        &self,
        system_pt: Option<&str>,
        user_pt: &str,
        n: u8,
        stream: bool,
    ) -> Result<(Vec<String>, u32, u32), Box<dyn std::error::Error>> {
        let config = OpenAIConfig::new()
            .with_api_base(&self.config.base)
            .with_api_key(&self.config.key);
        let client = Client::with_config(config);
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
        let config = OpenAIConfig::new()
            .with_api_base(&self.config.base)
            .with_api_key(&self.config.key);
        let client = Client::with_config(config);
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

    #[tokio::test]
    async fn test_llm() {
        let llm = LLM::new().unwrap();
        let sys_pt = "Your name is John";
        let user_pt = "Hello, what is your name?";
        let answers = llm
            .fetch_answer(Some(sys_pt), user_pt, 1, false)
            .await
            .unwrap().0;
        assert_eq!(answers.len(), 1);
        // println!("{:?}", answers);
        let answers = llm
            .fetch_answer(Some(sys_pt), user_pt, 2, true)
            .await
            .unwrap().0;
        assert_eq!(answers.len(), 2);
        // println!("{:?}", answers);
    }
}
