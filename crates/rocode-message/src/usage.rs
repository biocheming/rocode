use serde::{Deserialize, Serialize};

/// Usage statistics for a single assistant message.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct MessageUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_tokens: u64,
    pub cache_write_tokens: u64,
    pub cache_read_tokens: u64,
    pub total_cost: f64,
}

impl MessageUsage {
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens + self.output_tokens + self.reasoning_tokens
    }

    pub fn is_zero(&self) -> bool {
        self.input_tokens == 0
            && self.output_tokens == 0
            && self.reasoning_tokens == 0
            && self.cache_write_tokens == 0
            && self.cache_read_tokens == 0
            && self.total_cost == 0.0
    }
}
