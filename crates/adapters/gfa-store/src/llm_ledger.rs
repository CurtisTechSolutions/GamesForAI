//! Shared transaction logic; each backend supplies SQLx-checked literal queries.
use gfa_service::llm_ledger::*;

pub(crate) fn encode<T: serde::Serialize>(value: &T) -> Result<String, LedgerError> {
    serde_json::to_string(value).map_err(|_| LedgerError::InvalidInput)
}
pub(crate) fn call(payload: &str) -> Result<StoredLlmCall, LedgerError> {
    let call: StoredLlmCall =
        serde_json::from_str(payload).map_err(|_| LedgerError::Unavailable)?;
    if call
        .reservation
        .clone()
        .normalize()
        .map_err(|_| LedgerError::Unavailable)?
        != call.reservation
    {
        return Err(LedgerError::Unavailable);
    }
    Ok(call)
}
pub(crate) fn account(id: &str, payload: &str) -> Result<BudgetAccount, LedgerError> {
    let account: BudgetAccount =
        serde_json::from_str(payload).map_err(|_| LedgerError::Unavailable)?;
    if account.scope.id != id
        || account.scope.limit.tokens == 0
        || account.scope.limit.cost_microusd == 0
        || account.scope.limit.validate().is_err()
        || account.spent.validate().is_err()
        || account.held.validate().is_err()
    {
        return Err(LedgerError::Unavailable);
    }
    Ok(account)
}
pub(crate) fn database(error: sqlx::Error) -> LedgerError {
    match error {
        sqlx::Error::Database(error) if error.is_unique_violation() => LedgerError::Conflict,
        sqlx::Error::Database(error) if error.is_foreign_key_violation() => LedgerError::NotFound,
        _ => LedgerError::Unavailable,
    }
}

macro_rules! implement {
    ($store:ident, $db:ty, $begin:literal,
     $account_insert:literal,$account_lock:literal,$account_read:literal,$account_update:literal,
     $call_read:literal,$call_slot:literal,$call_insert:literal,$call_update:literal,$calls:literal) => {
        use crate::llm_ledger as ledger;
        use gfa_service::llm_ledger::*;

        impl $store {
            async fn llm_lock_accounts(
                tx: &mut sqlx::Transaction<'_, $db>,
                scopes: &[BudgetScope],
            ) -> Result<Vec<BudgetAccount>, LedgerError> {
                let mut accounts = Vec::new();
                // Reservation normalization fixes lock order across keys, matches and runs.
                for scope in scopes {
                    let empty = BudgetAccount {
                        scope: scope.clone(),
                        spent: Charge::default(),
                        held: Charge::default(),
                    };
                    let payload = ledger::encode(&empty)?;
                    sqlx::query!($account_insert, scope.id, payload)
                        .execute(&mut **tx)
                        .await
                        .map_err(ledger::database)?;
                    let row = sqlx::query!($account_lock, scope.id)
                        .fetch_one(&mut **tx)
                        .await
                        .map_err(ledger::database)?;
                    let account = ledger::account(&scope.id, &row.payload)?;
                    if account.scope != *scope {
                        return Err(LedgerError::Conflict);
                    }
                    accounts.push(account);
                }
                Ok(accounts)
            }
            async fn llm_write_accounts(
                tx: &mut sqlx::Transaction<'_, $db>,
                accounts: &[BudgetAccount],
            ) -> Result<(), LedgerError> {
                for account in accounts {
                    let payload = ledger::encode(account)?;
                    let changed = sqlx::query!($account_update, payload, account.scope.id)
                        .execute(&mut **tx)
                        .await
                        .map_err(ledger::database)?;
                    if changed.rows_affected() != 1 {
                        return Err(LedgerError::Unavailable);
                    }
                }
                Ok(())
            }
            async fn llm_read_call(
                tx: &mut sqlx::Transaction<'_, $db>,
                id: &str,
            ) -> Result<Option<StoredLlmCall>, LedgerError> {
                let row = sqlx::query!($call_read, id)
                    .fetch_optional(&mut **tx)
                    .await
                    .map_err(ledger::database)?;
                let call = row.map(|row| ledger::call(&row.payload)).transpose()?;
                if call.as_ref().is_some_and(|call| call.reservation.id != id) {
                    return Err(LedgerError::Unavailable);
                }
                Ok(call)
            }
            async fn llm_write_call(
                tx: &mut sqlx::Transaction<'_, $db>,
                call: &StoredLlmCall,
            ) -> Result<(), LedgerError> {
                let payload = ledger::encode(call)?;
                let changed = sqlx::query!($call_update, payload, call.reservation.id)
                    .execute(&mut **tx)
                    .await
                    .map_err(ledger::database)?;
                if changed.rows_affected() != 1 {
                    return Err(LedgerError::Unavailable);
                }
                Ok(())
            }
        }
        impl LlmLedger for $store {
            fn reserve(&self, reservation: CallReservation) -> LedgerFuture<'_, ReserveResult> {
                Box::pin(async move {
                    let reservation = reservation.normalize()?;
                    let mut tx = self
                        .pool
                        .begin_with($begin)
                        .await
                        .map_err(ledger::database)?;
                    let accounts = Self::llm_lock_accounts(&mut tx, &reservation.scopes).await?;
                    if let Some(existing) = Self::llm_read_call(&mut tx, &reservation.id).await? {
                        if existing.reservation != reservation {
                            return Err(LedgerError::Conflict);
                        }
                        tx.commit().await.map_err(ledger::database)?;
                        return Ok(ReserveResult::Existing(Box::new(existing)));
                    }
                    let seat = i32::from(reservation.seat);
                    let turn = reservation.turn as i64;
                    let attempt = reservation.attempt as i32;
                    let existing =
                        sqlx::query!($call_slot, reservation.match_id, seat, turn, attempt)
                            .fetch_optional(&mut *tx)
                            .await
                            .map_err(ledger::database)?;
                    if existing.is_some() {
                        return Err(LedgerError::Conflict);
                    }
                    let changed = accounts
                        .iter()
                        .map(|account| account.reserve(reservation.reserved))
                        .collect::<Result<Vec<_>, _>>()?;
                    let call = StoredLlmCall {
                        reservation: reservation.clone(),
                        state: CallState::Reserved,
                    };
                    let payload = ledger::encode(&call)?;
                    sqlx::query!(
                        $call_insert,
                        reservation.id,
                        reservation.match_id,
                        seat,
                        turn,
                        attempt,
                        payload
                    )
                    .execute(&mut *tx)
                    .await
                    .map_err(ledger::database)?;
                    Self::llm_write_accounts(&mut tx, &changed).await?;
                    tx.commit().await.map_err(ledger::database)?;
                    Ok(ReserveResult::Reserved)
                })
            }
            fn settle<'a>(
                &'a self,
                id: &'a str,
                response: ProviderResponse,
            ) -> LedgerFuture<'a, StoredLlmCall> {
                Box::pin(async move {
                    let mut tx = self
                        .pool
                        .begin_with($begin)
                        .await
                        .map_err(ledger::database)?;
                    let initial = Self::llm_read_call(&mut tx, id)
                        .await?
                        .ok_or(LedgerError::NotFound)?;
                    let accounts =
                        Self::llm_lock_accounts(&mut tx, &initial.reservation.scopes).await?;
                    // Re-read after the account locks: another settler may have committed.
                    let current = Self::llm_read_call(&mut tx, id)
                        .await?
                        .ok_or(LedgerError::NotFound)?;
                    let settled = current.settle(response)?;
                    if !matches!(current.state, CallState::Settled { .. }) {
                        let CallState::Settled { charged, .. } = &settled.state else {
                            return Err(LedgerError::Unavailable);
                        };
                        let changed = accounts
                            .iter()
                            .map(|account| account.settle(current.reservation.reserved, *charged))
                            .collect::<Result<Vec<_>, _>>()?;
                        Self::llm_write_accounts(&mut tx, &changed).await?;
                        Self::llm_write_call(&mut tx, &settled).await?;
                    }
                    tx.commit().await.map_err(ledger::database)?;
                    Ok(settled)
                })
            }
            fn mark_uncertain<'a>(
                &'a self,
                id: &'a str,
                error: ProviderError,
            ) -> LedgerFuture<'a, StoredLlmCall> {
                Box::pin(async move {
                    let mut tx = self
                        .pool
                        .begin_with($begin)
                        .await
                        .map_err(ledger::database)?;
                    let initial = Self::llm_read_call(&mut tx, id)
                        .await?
                        .ok_or(LedgerError::NotFound)?;
                    Self::llm_lock_accounts(&mut tx, &initial.reservation.scopes).await?;
                    let mut current = Self::llm_read_call(&mut tx, id)
                        .await?
                        .ok_or(LedgerError::NotFound)?;
                    if matches!(current.state, CallState::Reserved) {
                        current.state = CallState::Uncertain { error };
                        Self::llm_write_call(&mut tx, &current).await?;
                    }
                    tx.commit().await.map_err(ledger::database)?;
                    Ok(current)
                })
            }
            fn load_call<'a>(&'a self, id: &'a str) -> LedgerFuture<'a, Option<StoredLlmCall>> {
                Box::pin(async move {
                    let row = sqlx::query!($call_read, id)
                        .fetch_optional(&self.pool)
                        .await
                        .map_err(ledger::database)?;
                    let call = row.map(|row| ledger::call(&row.payload)).transpose()?;
                    if call.as_ref().is_some_and(|call| call.reservation.id != id) {
                        return Err(LedgerError::Unavailable);
                    }
                    Ok(call)
                })
            }
            fn account<'a>(&'a self, id: &'a str) -> LedgerFuture<'a, Option<BudgetAccount>> {
                Box::pin(async move {
                    let row = sqlx::query!($account_read, id)
                        .fetch_optional(&self.pool)
                        .await
                        .map_err(ledger::database)?;
                    row.map(|row| ledger::account(id, &row.payload)).transpose()
                })
            }
            fn calls<'a>(
                &'a self,
                match_id: &'a str,
                seat: u8,
                after: Option<CallCursor>,
                limit: u32,
            ) -> LedgerFuture<'a, Vec<StoredLlmCall>> {
                Box::pin(async move {
                    if !(1..=100).contains(&limit)
                        || after.is_some_and(|cursor| {
                            cursor.turn > i64::MAX as u64 || cursor.attempt > 1000
                        })
                    {
                        return Err(LedgerError::InvalidInput);
                    }
                    let seat = i32::from(seat);
                    let turn = after.map_or(-1, |cursor| cursor.turn as i64);
                    let attempt = after.map_or(-1, |cursor| cursor.attempt as i32);
                    let limit = i64::from(limit);
                    let query = sqlx::query!($calls, match_id, seat, turn, turn, attempt, limit);
                    let mut rows = query.fetch(&self.pool);
                    let mut result = Vec::new();
                    let mut bytes = 0_usize;
                    while let Some(row) = futures_util::TryStreamExt::try_next(&mut rows)
                        .await
                        .map_err(ledger::database)?
                    {
                        // Return a larger single record whole; never skip it when paging.
                        if !result.is_empty() && row.payload.len() > 8 * 1024 * 1024 - bytes {
                            break;
                        }
                        let call = ledger::call(&row.payload)?;
                        if call.reservation.match_id != match_id
                            || i32::from(call.reservation.seat) != seat
                        {
                            return Err(LedgerError::Unavailable);
                        }
                        bytes += row.payload.len();
                        result.push(call);
                        if bytes >= 8 * 1024 * 1024 {
                            break;
                        }
                    }
                    Ok(result)
                })
            }
        }
    };
}
pub(crate) use implement;
