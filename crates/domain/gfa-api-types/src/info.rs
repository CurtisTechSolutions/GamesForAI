//! Shared ordered briefing format for JSON and prompt-ready Markdown.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Amount of briefing detail.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InfoDetail {
    /// Rules and essential examples without schemas or strategy hints.
    Compact,
    /// All schemas, examples, and optional strategy notes.
    #[default]
    Full,
}

/// One named section, in the order specified by the model briefing contract.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct InfoSection {
    /// Stable section identifier, such as identity or initial_state.
    pub id: String,
    /// Human-readable explanation, including explicit absence statements.
    pub text: String,
    /// Structured facts, examples, or schemas supplementing the explanation.
    pub data: Value,
}

/// Transport-independent model briefing, rendered from one canonical data structure.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Briefing {
    /// Engine information version plus the briefing format revision.
    pub info_version: String,
    /// Heuristic upper estimate across JSON and Markdown; not tokenizer-specific.
    pub approx_tokens: usize,
    /// Required sections in their stable presentation order.
    pub sections: Vec<InfoSection>,
}

impl Briefing {
    /// Render every section and structured fact without fetching other documents.
    pub fn markdown(&self) -> Result<String, serde_json::Error> {
        let mut result = format!(
            "Info version: {}\nApproximate tokens: {}\n",
            self.info_version, self.approx_tokens
        );
        for section in &self.sections {
            result.push_str(&format!("\n## {}\n\n{}\n", section.id, section.text));
            if !section.data.is_null() {
                // A longer fence keeps user-supplied position strings inside JSON.
                let data = serde_json::to_string(&section.data)?;
                let longest = data.split(|c| c != '`').map(str::len).max().unwrap_or(0);
                let fence = "`".repeat(longest.max(2) + 1);
                result.push_str(&format!("\n{fence}json\n{data}\n{fence}\n"));
            }
        }
        Ok(result)
    }

    /// Refresh the rough byte-based token estimate after composing all sections.
    pub fn estimate_tokens(&mut self) -> Result<(), serde_json::Error> {
        self.approx_tokens = 0;
        let bytes = serde_json::to_vec(self)?.len().max(self.markdown()?.len());
        // Reserve room for the estimate's own digits in both representations.
        self.approx_tokens = (bytes + 20).div_ceil(4);
        Ok(())
    }
}
