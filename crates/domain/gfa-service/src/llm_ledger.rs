//! Transactional accounting boundary for paid model calls.
//! Account identifiers are host-generated references, never provider credentials.
pub use gfa_llm::{Pricing, ProviderError, ProviderRequest, ProviderResponse};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, future::Future, pin::Pin};

/// Failures from a durable call journal; no provider or SQL details are exposed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum LedgerError {
    /// Malformed identifiers, limits, or accounting data.
    #[error("invalid LLM ledger input")]
    InvalidInput,
    /// A call identifier, attempt, or account configuration changed.
    #[error("LLM ledger conflict")]
    Conflict,
    /// A referenced call or match is absent.
    #[error("LLM ledger record not found")]
    NotFound,
    /// At least one shared scope cannot afford the requested reservation.
    #[error("LLM budget exhausted")]
    BudgetExhausted,
    /// Persistence could not complete; callers must not dispatch a paid call.
    #[error("LLM ledger unavailable")]
    Unavailable,
}

/// Checked billable token count and integer micro-USD amount.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Charge {
    /// Combined input, output and cache tokens.
    pub tokens: u64,
    /// Millionths of one USD.
    pub cost_microusd: u64,
}
impl Charge {
    /// Storage integers are signed 64-bit on both supported backends.
    pub fn validate(self) -> Result<(), LedgerError> {
        if self.tokens > i64::MAX as u64 || self.cost_microusd > i64::MAX as u64 {
            return Err(LedgerError::InvalidInput);
        }
        Ok(())
    }
    fn add(self, rhs: Self) -> Result<Self, LedgerError> {
        let sum = Self {
            tokens: self
                .tokens
                .checked_add(rhs.tokens)
                .ok_or(LedgerError::InvalidInput)?,
            cost_microusd: self
                .cost_microusd
                .checked_add(rhs.cost_microusd)
                .ok_or(LedgerError::InvalidInput)?,
        };
        sum.validate()?;
        Ok(sum)
    }
    fn subtract(self, rhs: Self) -> Result<Self, LedgerError> {
        Ok(Self {
            tokens: self
                .tokens
                .checked_sub(rhs.tokens)
                .ok_or(LedgerError::InvalidInput)?,
            cost_microusd: self
                .cost_microusd
                .checked_sub(rhs.cost_microusd)
                .ok_or(LedgerError::InvalidInput)?,
        })
    }
}

/// One immutable budget shared across calls, games, or benchmark tasks.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetScope {
    /// Host reference, prefixed match:, key:, or run:. Never store an API key here.
    pub id: String,
    /// Lifetime limit for this account; use a new reference for a new budget period.
    pub limit: Charge,
}

/// Persisted balance including calls with still-unknown billing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetAccount {
    /// Identity and immutable limit.
    pub scope: BudgetScope,
    /// Known provider usage and cost.
    pub spent: Charge,
    /// Reservations for pending or uncertain calls.
    pub held: Charge,
}
impl BudgetAccount {
    /// Check and reserve without changing the original account on failure.
    pub fn reserve(&self, charge: Charge) -> Result<Self, LedgerError> {
        charge.validate()?;
        let total = self.spent.add(self.held)?.add(charge)?;
        if total.tokens > self.scope.limit.tokens
            || total.cost_microusd > self.scope.limit.cost_microusd
        {
            return Err(LedgerError::BudgetExhausted);
        }
        Ok(Self {
            held: self.held.add(charge)?,
            ..self.clone()
        })
    }
    /// Replace a reservation with actual billed usage, even if a host underestimated.
    /// Accurate overspend remains visible and blocks subsequent reservations.
    pub fn settle(&self, reserved: Charge, actual: Charge) -> Result<Self, LedgerError> {
        actual.validate()?;
        Ok(Self {
            held: self.held.subtract(reserved)?,
            spent: self.spent.add(actual)?,
            ..self.clone()
        })
    }
}

/// One idempotent, bounded dispatch authorization, journaled before HTTP.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CallReservation {
    /// Stable idempotency key chosen by the host for this call.
    pub id: String,
    /// Persisted match owning the transcript.
    pub match_id: String,
    /// Authorized player seat.
    pub seat: u8,
    /// Match turn at which the call was requested.
    pub turn: u64,
    /// Zero-based provider-call number in that turn, including planning and retries.
    pub attempt: u32,
    /// Complete credential-free request.
    pub request: ProviderRequest,
    /// Immutable host pricing used for settlement and reports.
    pub pricing: Pricing,
    /// Required match/key scopes and optional benchmark run scope.
    pub scopes: Vec<BudgetScope>,
    /// Conservative upper bound reserved across every scope before dispatch.
    pub reserved: Charge,
    /// Host clock in Unix milliseconds.
    pub created_at_ms: u64,
}
fn identifier(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_:./".contains(&b))
}
impl CallReservation {
    /// Validate and sort scopes for stable equality and deadlock-free row locking.
    pub fn normalize(mut self) -> Result<Self, LedgerError> {
        self.request
            .config
            .validate()
            .map_err(|_| LedgerError::InvalidInput)?;
        self.reserved.validate()?;
        if !identifier(&self.id, 128)
            || !identifier(&self.match_id, 128)
            || self.turn > i64::MAX as u64
            || self.created_at_ms > i64::MAX as u64
            || self.attempt > 1000
            || !(2..=3).contains(&self.scopes.len())
            || self.reserved.tokens < u64::from(self.request.config.max_output_tokens)
            || self.reserved.cost_microusd == 0
            || !identifier(&self.pricing.version, 128)
            || serde_json::to_vec(&self.request)
                .map_err(|_| LedgerError::InvalidInput)?
                .len()
                > 4 * 1024 * 1024
        {
            return Err(LedgerError::InvalidInput);
        }
        self.scopes.sort_by(|a, b| a.id.cmp(&b.id));
        let mut kinds = BTreeSet::new();
        for scope in &self.scopes {
            scope.limit.validate()?;
            let (kind, id) = scope.id.split_once(':').ok_or(LedgerError::InvalidInput)?;
            if !matches!(kind, "match" | "run" | "key")
                || !identifier(id, 128)
                || !kinds.insert(kind)
                || (kind == "match" && id != self.match_id)
                || scope.limit.tokens == 0
                || scope.limit.cost_microusd == 0
            {
                return Err(LedgerError::InvalidInput);
            }
        }
        if !kinds.contains("match") || !kinds.contains("key") {
            return Err(LedgerError::InvalidInput);
        }
        Ok(self)
    }
}

/// A reservation is never released just because the caller lost its response.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum CallState {
    /// Dispatch was authorized; only the caller that created it may send HTTP.
    Reserved,
    /// Billing is unknown. Keep the full reservation until explicit reconciliation.
    Uncertain {
        /// Sanitized transport/cancellation failure.
        error: ProviderError,
    },
    /// Provider usage was recorded and the reservation was settled exactly once.
    Settled {
        /// Unmodified provider response for audit and recovery.
        response: Box<ProviderResponse>,
        /// Price calculated from the call's immutable pricing snapshot.
        charged: Charge,
        /// The caller's bound was too low; report it and block exhausted accounts.
        exceeded_reservation: bool,
    },
}

/// Complete stored provider-call journal entry.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoredLlmCall {
    /// Original dispatch request and all scopes.
    pub reservation: CallReservation,
    /// Accounting and response status.
    pub state: CallState,
}
impl StoredLlmCall {
    /// Calculate settlement with immutable pricing; repeated identical input is safe.
    pub fn settle(&self, response: ProviderResponse) -> Result<Self, LedgerError> {
        if serde_json::to_vec(&response)
            .map_err(|_| LedgerError::InvalidInput)?
            .len()
            > 9 * 1024 * 1024
        {
            return Err(LedgerError::InvalidInput);
        }
        if let CallState::Settled {
            response: previous, ..
        } = &self.state
        {
            return if previous.as_ref() == &response {
                Ok(self.clone())
            } else {
                Err(LedgerError::Conflict)
            };
        }
        let charged = Charge {
            tokens: response
                .usage
                .total()
                .map_err(|_| LedgerError::InvalidInput)?,
            cost_microusd: self
                .reservation
                .pricing
                .cost(response.usage)
                .map_err(|_| LedgerError::InvalidInput)?,
        };
        charged.validate()?;
        Ok(Self {
            reservation: self.reservation.clone(),
            state: CallState::Settled {
                exceeded_reservation: charged.tokens > self.reservation.reserved.tokens
                    || charged.cost_microusd > self.reservation.reserved.cost_microusd,
                response: Box::new(response),
                charged,
            },
        })
    }
}

/// Only a new reservation authorizes one paid API dispatch.
#[derive(Clone, Debug, PartialEq)]
pub enum ReserveResult {
    /// Newly journaled and reserved atomically across every scope.
    Reserved,
    /// Already authorized earlier; reuse the response or retain uncertain billing.
    /// This result never authorizes sending the HTTP request again.
    Existing(Box<StoredLlmCall>),
}

/// Cursor ordered by game turn and provider attempt within one seat's transcript.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CallCursor {
    /// Last returned turn.
    pub turn: u64,
    /// Last returned call number in the turn.
    pub attempt: u32,
}
/// Async durable ledger operation independent of a database implementation.
pub type LedgerFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, LedgerError>> + Send + 'a>>;

/// Persistence port. Reservations, settlements, and all affected account balances
/// must commit together. Backends must serialize shared account changes.
pub trait LlmLedger: Send + Sync {
    /// Reserve all scopes and write the request, or change nothing on failure.
    fn reserve(&self, reservation: CallReservation) -> LedgerFuture<'_, ReserveResult>;
    /// Settle reported usage once. Only a trusted host may reconcile uncertain calls.
    fn settle<'a>(
        &'a self,
        id: &'a str,
        response: ProviderResponse,
    ) -> LedgerFuture<'a, StoredLlmCall>;
    /// Preserve the reservation and record the failure. Settled calls stay settled.
    fn mark_uncertain<'a>(
        &'a self,
        id: &'a str,
        error: ProviderError,
    ) -> LedgerFuture<'a, StoredLlmCall>;
    /// Read a known call without authorizing any new dispatch.
    fn load_call<'a>(&'a self, id: &'a str) -> LedgerFuture<'a, Option<StoredLlmCall>>;
    /// Read one account's consistent known/held balance.
    fn account<'a>(&'a self, id: &'a str) -> LedgerFuture<'a, Option<BudgetAccount>>;
    /// Ordered page of at most 100 calls, for a host-authorized seat only.
    fn calls<'a>(
        &'a self,
        match_id: &'a str,
        seat: u8,
        after: Option<CallCursor>,
        limit: u32,
    ) -> LedgerFuture<'a, Vec<StoredLlmCall>>;
}
