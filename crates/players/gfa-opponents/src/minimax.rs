use super::*;

type SearchResult = Result<Option<(f64, Vec<String>)>, GameError>;

pub(super) fn choose(
    game: &dyn DynGame,
    state: &Value,
    turn: &PlayerTurn<'_>,
    budget: &mut Budget<'_>,
) -> Result<ActionChoice, GameError> {
    let fallback = turn.legal_actions.first()
        .ok_or_else(|| GameError::illegal("No legal action"))?;
    let mut result = choice(fallback.clone(), "minimax");
    for depth in 1..=budget.limits.depth {
        let Some((score, variation)) = search(
            game, state, turn.seat, depth, (f64::NEG_INFINITY, f64::INFINITY), budget,
        )? else { break; };
        if let Some(action) = turn.legal_actions.iter()
            .find(|action| variation.first() == Some(&action.string))
        {
            result.action = action.clone();
            result.info.principal_variation = variation;
            result.info.evaluation = Some(score);
            result.info.depth = depth;
        }
    }
    result.info.nodes = budget.nodes;
    result.info.budget_exhausted = budget.exhausted;
    Ok(result)
}

fn search(
    game: &dyn DynGame,
    state: &Value,
    root: PlayerId,
    depth: u8,
    (mut alpha, mut beta): (f64, f64),
    budget: &mut Budget<'_>,
) -> SearchResult {
    if !budget.enter() {
        return Ok(None);
    }
    if depth == 0 || game.is_terminal(state)? {
        return Ok(Some((payoff(game, state, root)?, vec![])));
    }
    let seat = actor(game, state)?;
    let maximizing = seat == root;
    let mut best = if maximizing { f64::NEG_INFINITY } else { f64::INFINITY };
    let mut variation = Vec::new();
    let actions = game.legal_actions(state, seat)?;
    if actions.is_empty() {
        return Err(GameError::illegal("Non-terminal engine position has no legal moves"));
    }
    for action in actions {
        let (next, _) = game.apply(state, seat, &action.json)?;
        let Some((score, mut child_line)) = search(
            game, &next, root, depth - 1, (alpha, beta), budget,
        )? else { return Ok(None); };
        if (maximizing && score > best) || (!maximizing && score < best) {
            best = score;
            child_line.insert(0, action.string);
            variation = child_line;
        }
        if maximizing { alpha = alpha.max(best); } else { beta = beta.min(best); }
        if beta <= alpha {
            break;
        }
    }
    Ok(Some((best, variation)))
}
