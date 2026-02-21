use std::collections::{BTreeMap, HashMap};

use clap::ValueEnum;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaintLevel {
    Clean,
    Tainted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaintSpan {
    pub source: String,
    pub detail: String,
    pub digest: String,
}

pub type MessageId = usize;

#[derive(Debug, Clone, Default)]
pub struct TaintState {
    pub message_taints: HashMap<MessageId, Vec<TaintSpan>>,
    pub spans_by_tool_call_id: BTreeMap<String, Vec<TaintSpan>>,
    pub overall: TaintLevel,
    pub last_sources: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize, Deserialize)]
pub enum TaintToggle {
    Off,
    On,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize, Deserialize)]
pub enum TaintMode {
    Propagate,
    PropagateAndEnforce,
}

impl TaintState {
    pub fn new() -> Self {
        Self {
            message_taints: HashMap::new(),
            spans_by_tool_call_id: BTreeMap::new(),
            overall: TaintLevel::Clean,
            last_sources: Vec::new(),
        }
    }

    pub fn add_tool_spans(&mut self, tool_call_id: &str, message_id: MessageId, spans: Vec<TaintSpan>) {
        if spans.is_empty() {
            return;
        }
        self.overall = TaintLevel::Tainted;
        self.last_sources = spans.iter().map(|s| s.source.clone()).collect();
        self.message_taints
            .entry(message_id)
            .or_default()
            .extend(spans.clone());
        self.spans_by_tool_call_id
            .entry(tool_call_id.to_string())
            .or_default()
            .extend(spans);
    }

    pub fn mark_assistant_context_tainted(&mut self, message_id: MessageId) {
        if !matches!(self.overall, TaintLevel::Tainted) {
            return;
        }
        self.message_taints.entry(message_id).or_default().push(TaintSpan {
            source: "other".to_string(),
            detail: "tainted_context".to_string(),
            digest: String::new(),
        });
    }
}

pub fn digest_prefix_hex(content: &str, digest_bytes: usize) -> String {
    let bytes = content.as_bytes();
    let take = bytes.len().min(digest_bytes);
    crate::store::sha256_hex(&bytes[..take])
}

#[cfg(test)]
mod tests {
    use super::digest_prefix_hex;

    #[test]
    fn digest_prefix_is_deterministic() {
        let a = digest_prefix_hex("abcdef", 3);
        let b = digest_prefix_hex("abczzz", 3);
        assert_eq!(a, b);
    }
}
