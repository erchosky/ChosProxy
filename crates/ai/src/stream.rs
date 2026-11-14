//! Reading a model's answer while it is written (M31b): Ollama sends one
//! JSON object per line, OpenAI-compatible servers send Server-Sent Events
//! (`data: {…}` lines, then `data: [DONE]`). Pure: bytes in, text out.

use crate::{ChatMessage, parse_reply};
use chosproxy_core::config::AiProvider;
use serde_json::{Value, json};

/// Most bytes of an answer kept for the non-streaming fallback.
const MAX_RAW: usize = 4 * 1024 * 1024;

/// The JSON body of a chat request that asks for a streamed answer.
pub fn stream_body(provider: AiProvider, model: &str, messages: &[ChatMessage]) -> String {
    let msgs: Vec<Value> = messages
        .iter()
        .map(|m| json!({ "role": m.role, "content": m.content }))
        .collect();
    match provider {
        AiProvider::Ollama => json!({ "model": model, "messages": msgs, "stream": true }),
        AiProvider::OpenAiCompatible => {
            json!({ "model": model, "messages": msgs, "stream": true, "temperature": 0.2 })
        }
    }
    .to_string()
}

/// Turns the bytes of a streamed answer into text, piece by piece.
#[derive(Debug)]
pub struct StreamParser {
    provider: AiProvider,
    /// Bytes of a line that is not complete yet.
    line: Vec<u8>,
    /// Everything received, for a server that did not stream.
    raw: Vec<u8>,
    text: String,
    error: Option<String>,
    streamed: bool,
}

impl StreamParser {
    /// A parser for one answer.
    pub fn new(provider: AiProvider) -> StreamParser {
        StreamParser {
            provider,
            line: Vec::new(),
            raw: Vec::new(),
            text: String::new(),
            error: None,
            streamed: false,
        }
    }

    /// Adds body bytes; returns the answer text they completed.
    pub fn feed(&mut self, bytes: &[u8]) -> String {
        let room = MAX_RAW.saturating_sub(self.raw.len());
        self.raw.extend_from_slice(&bytes[..bytes.len().min(room)]);
        let mut delta = String::new();
        for b in bytes {
            if *b == b'\n' {
                let line = std::mem::take(&mut self.line);
                self.take_line(&line, &mut delta);
            } else if self.line.len() < MAX_RAW {
                self.line.push(*b);
            }
        }
        self.text.push_str(&delta);
        delta
    }

    fn take_line(&mut self, line: &[u8], delta: &mut String) {
        let Ok(line) = std::str::from_utf8(line) else {
            return;
        };
        let line = line.trim();
        let payload = match self.provider {
            AiProvider::Ollama => line,
            AiProvider::OpenAiCompatible => match line.strip_prefix("data:") {
                Some(p) => p.trim(),
                None => return,
            },
        };
        if payload.is_empty() || payload == "[DONE]" {
            return;
        }
        let Ok(value) = serde_json::from_str::<Value>(payload) else {
            return;
        };
        if let Some(e) = value.get("error") {
            let message = e
                .get("message")
                .and_then(Value::as_str)
                .or_else(|| e.as_str())
                .unwrap_or("unknown error");
            self.error = Some(format!("the model server reported: {message}"));
            return;
        }
        let piece = match self.provider {
            AiProvider::Ollama => value.pointer("/message/content"),
            AiProvider::OpenAiCompatible => value.pointer("/choices/0/delta/content"),
        }
        .and_then(Value::as_str);
        if let Some(piece) = piece {
            self.streamed = true;
            delta.push_str(piece);
        }
    }

    /// The text so far.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The whole answer at the end of the body. A server that answered
    /// without streaming (one JSON document) is read like a normal reply.
    pub fn finish(mut self) -> Result<String, String> {
        if !self.line.is_empty() {
            let line = std::mem::take(&mut self.line);
            let mut delta = String::new();
            self.take_line(&line, &mut delta);
            self.text.push_str(&delta);
        }
        if let Some(e) = self.error {
            return Err(e);
        }
        if self.streamed {
            return Ok(self.text.trim().to_string());
        }
        parse_reply(self.provider, &String::from_utf8_lossy(&self.raw))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;

    #[test]
    fn reads_ollama_lines_split_anywhere() {
        let mut p = StreamParser::new(AiProvider::Ollama);
        let body = "{\"message\":{\"content\":\"Hel\"},\"done\":false}\n\
                    {\"message\":{\"content\":\"lo ñ\"},\"done\":false}\n\
                    {\"message\":{\"content\":\"\"},\"done\":true}\n";
        let mut seen = String::new();
        for chunk in body.as_bytes().chunks(7) {
            seen.push_str(&p.feed(chunk));
        }
        assert_eq!(seen, "Hello ñ");
        assert_eq!(p.text(), "Hello ñ");
        assert_eq!(p.finish().unwrap(), "Hello ñ");
    }

    #[test]
    fn reads_openai_events_and_ignores_the_rest() {
        let mut p = StreamParser::new(AiProvider::OpenAiCompatible);
        let d = p.feed(
            b": keep-alive\r\n\r\ndata: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\r\n\r\n\
              data: {\"choices\":[{\"delta\":{\"content\":\"Yo\"}}]}\r\n\r\ndata: [DONE]\r\n\r\n",
        );
        assert_eq!(d, "Yo");
        assert_eq!(p.finish().unwrap(), "Yo");
    }

    #[test]
    fn falls_back_to_a_whole_reply_and_reports_errors() {
        let mut p = StreamParser::new(AiProvider::OpenAiCompatible);
        assert_eq!(
            p.feed(b"{\"choices\":[{\"message\":{\"content\":\"Whole\"}}]}"),
            ""
        );
        assert_eq!(p.finish().unwrap(), "Whole");
        // Ollama's whole reply has the shape of one stream line.
        let mut p = StreamParser::new(AiProvider::Ollama);
        p.feed(b"{\"message\":{\"content\":\" One \"},\"done\":true}");
        assert_eq!(p.finish().unwrap(), "One");
        let mut p = StreamParser::new(AiProvider::Ollama);
        p.feed(b"{\"message\":{\"content\":\"a\"}}\n{\"error\":\"out of memory\"}\n");
        assert!(p.finish().unwrap_err().contains("out of memory"));
        let mut p = StreamParser::new(AiProvider::OpenAiCompatible);
        p.feed(b"data: {\"error\":{\"message\":\"bad key\"}}\n\n");
        assert!(p.finish().unwrap_err().contains("bad key"));
        assert!(StreamParser::new(AiProvider::Ollama).finish().is_err());
        let b: Value = serde_json::from_str(&stream_body(AiProvider::Ollama, "m", &[])).unwrap();
        assert_eq!(b["stream"], true);
    }
}
