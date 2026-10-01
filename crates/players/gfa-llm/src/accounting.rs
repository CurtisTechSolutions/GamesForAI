use crate::ProviderError;
use serde::{Deserialize, Serialize};

/// Mutually exclusive token classes; cache tokens are not counted as uncached input.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Usage {
    /// Uncached input tokens.
    pub input_tokens: u64,
    /// Output tokens, including provider thinking tokens.
    pub output_tokens: u64,
    /// Tokens read from the prompt cache.
    pub cache_read_input_tokens: u64,
    /// Tokens written with a five-minute cache lifetime.
    pub cache_creation_5m_tokens: u64,
    /// Tokens written with a one-hour cache lifetime.
    pub cache_creation_1h_tokens: u64,
}
impl Usage {
    /// Total consumed tokens, rejecting overflow instead of wrapping the budget.
    pub fn total(self) -> Result<u64, ProviderError> {
        self.counts().into_iter().try_fold(0_u64, |sum, count| {
            sum.checked_add(count).ok_or(ProviderError::InvalidResponse)
        })
    }
    fn counts(self) -> [u64; 5] {
        [
            self.input_tokens,
            self.output_tokens,
            self.cache_read_input_tokens,
            self.cache_creation_5m_tokens,
            self.cache_creation_1h_tokens,
        ]
    }
}

/// Explicit, versioned host pricing. Rates are micro-USD per million tokens.
/// Model pricing changes independently of model identifiers; do not infer it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pricing {
    /// Identifier recorded with every estimate (for example a pricing date).
    pub version: String,
    /// Uncached input price.
    pub input: u64,
    /// Output price.
    pub output: u64,
    /// Cache read price.
    pub cache_read: u64,
    /// Five-minute cache write price.
    pub cache_write_5m: u64,
    /// One-hour cache write price.
    pub cache_write_1h: u64,
}
impl Pricing {
    /// Estimate cost, rounding upward once to the next micro-USD.
    pub fn cost(&self, usage: Usage) -> Result<u64, ProviderError> {
        usage.total()?;
        let rates = [
            self.input,
            self.output,
            self.cache_read,
            self.cache_write_5m,
            self.cache_write_1h,
        ];
        let numerator = usage.counts().into_iter().zip(rates).try_fold(
            0_u128,
            |sum, (tokens, rate)| {
                sum.checked_add(u128::from(tokens) * u128::from(rate))
                    .ok_or(ProviderError::InvalidResponse)
            },
        )?;
        u64::try_from(numerator.div_ceil(1_000_000))
            .map_err(|_| ProviderError::InvalidResponse)
    }
}

/// Pure budget arithmetic for a durable host ledger.
/// Hosts must serialize reservations and accounting across concurrent requests.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Budget {
    /// Maximum combined tokens.
    pub token_limit: u64,
    /// Maximum total micro-USD.
    pub cost_limit: u64,
    /// Tokens already charged or reserved.
    pub committed_tokens: u64,
    /// Micro-USD already charged or reserved.
    pub committed_cost: u64,
}
impl Budget {
    /// Check a proposed reservation without changing the ledger.
    pub fn check(&self, tokens: u64, cost: u64) -> Result<(), ProviderError> {
        let tokens = self.committed_tokens.checked_add(tokens);
        let cost = self.committed_cost.checked_add(cost);
        if tokens.is_none_or(|total| total > self.token_limit)
            || cost.is_none_or(|total| total > self.cost_limit)
        {
            return Err(ProviderError::BudgetExhausted);
        }
        Ok(())
    }
}
