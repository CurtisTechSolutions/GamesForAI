use super::*;

struct Node {
    state: Value,
    seat: Option<PlayerId>,
    pending: Vec<LegalAction>,
    children: Vec<(LegalAction, usize)>,
    visits: u64,
    sums: Vec<f64>,
}

fn node(
    game: &dyn DynGame,
    state: Value,
    rng: &mut SeededRng,
) -> Result<Node, GameError> {
    let seat = if game.is_terminal(&state)? { None } else { Some(actor(game, &state)?) };
    let mut pending = match seat {
        Some(seat) => game.legal_actions(&state, seat)?,
        None => vec![],
    };
    rng.shuffle(&mut pending);
    let players = game.returns(&state)?.len();
    Ok(Node { state, seat, pending, children: vec![], visits: 0, sums: vec![0.0; players] })
}

fn rewards(game: &dyn DynGame, state: &Value) -> Result<Vec<f64>, GameError> {
    let [low, high] = game.spec().reward_range;
    let values = game.returns(state)?;
    if values.iter().any(|value| !value.is_finite() || *value < low || *value > high) {
        return Err(GameError::illegal("Engine returned rewards outside its declared range"));
    }
    Ok(values.into_iter().map(|value| (value - low) / (high - low)).collect())
}

pub(super) fn choose(
    game: &dyn DynGame,
    state: &Value,
    turn: &PlayerTurn<'_>,
    budget: &mut Budget<'_>,
) -> Result<ActionChoice, GameError> {
    let fallback = turn.legal_actions.first()
        .ok_or_else(|| GameError::illegal("No legal action"))?;
    let mut result = choice(fallback.clone(), "mcts");
    let mut rng = SeededRng::new(budget.limits.seed);
    let mut tree = vec![node(game, state.clone(), &mut rng)?];
    while budget.enter() {
        let mut path = vec![0];
        let mut depth = 0;
        let mut current = 0;
        // Selection and one-node expansion.
        while depth < budget.limits.depth {
            let Some(seat) = tree[current].seat else { break; };
            if let Some(action) = tree[current].pending.pop() {
                if !budget.enter() { break; }
                let (next, _) = game.apply(&tree[current].state, seat, &action.json)?;
                let index = tree.len();
                tree.push(node(game, next, &mut rng)?);
                tree[current].children.push((action, index));
                current = index;
                path.push(current);
                depth += 1;
                break;
            }
            let parent_visits = tree[current].visits.max(1) as f64;
            let next = tree[current].children.iter().max_by(|(_, a), (_, b)| {
                let score = |index: usize| {
                    let child = &tree[index];
                    if child.visits == 0 {
                        f64::INFINITY
                    } else {
                        child.sums[usize::from(seat)] / child.visits as f64
                            + (2.0 * parent_visits.ln() / child.visits as f64).sqrt()
                    }
                };
                score(*a).total_cmp(&score(*b))
            }).map(|(_, index)| *index);
            let Some(next) = next else { break; };
            if !budget.enter() { break; }
            current = next;
            path.push(current);
            depth += 1;
        }
        if budget.exhausted { break; }
        result.info.depth = result.info.depth.max(depth);
        // Rollout uses the same per-seat engine rules, with independent randomness.
        let mut rollout = tree[current].state.clone();
        while depth < budget.limits.depth && !game.is_terminal(&rollout)? {
            if !budget.enter() { break; }
            let seat = actor(game, &rollout)?;
            let actions = game.legal_actions(&rollout, seat)?;
            let index = rng.index(actions.len())
                .ok_or_else(|| GameError::illegal("Non-terminal position has no legal moves"))?;
            rollout = game.apply(&rollout, seat, &actions[index].json)?.0;
            depth += 1;
        }
        if budget.exhausted { break; }
        let values = rewards(game, &rollout)?;
        for index in path {
            if values.len() != tree[index].sums.len() {
                return Err(GameError::illegal("Player count changed during search"));
            }
            tree[index].visits += 1;
            for (sum, value) in tree[index].sums.iter_mut().zip(&values) { *sum += value; }
        }
    }
    if let Some((action, index)) = tree[0].children.iter()
        .filter(|(_, index)| tree[*index].visits > 0)
        .max_by_key(|(_, index)| tree[*index].visits)
    {
        result.action = action.clone();
        result.info.principal_variation = vec![action.string.clone()];
        let [low, high] = game.spec().reward_range;
        result.info.evaluation = Some(
            low + tree[*index].sums[usize::from(turn.seat)] / tree[*index].visits as f64 * (high - low),
        );
    }
    result.info.nodes = budget.nodes;
    result.info.budget_exhausted = budget.exhausted;
    Ok(result)
}
