use super::*;

#[test]
fn conforms() -> Result<(), Box<dyn std::error::Error>> {
    gfa_testkit::conformance::<TicTacToe>()
}

#[test]
fn rejects_unreachable_positions() {
    for notation in [
        r#"{"board":[0,0,0,1,1,1,null,null,null],"to_move":0}"#,
        r#"{"board":[0,null,null,null,null,null,null,null,null],"to_move":0}"#,
        r#"{"board":[2,null,null,null,null,null,null,null,null],"to_move":1}"#,
    ] {
        assert!(TicTacToe::state_from_notation(&Config {}, notation).is_err());
    }
}

#[test]
fn errors_are_atomic_and_win_stops_play() -> Result<(), GameError> {
    let mut state = TicTacToe::new_initial_state(&Config {}, 0)?;
    for (seat, row, col) in [(0,1,1), (1,2,1), (0,1,2), (1,2,2)] {
        TicTacToe::apply(&mut state, seat, &Action { row, col })?;
    }
    let before = state.clone();
    assert!(TicTacToe::apply(&mut state, 1, &Action { row: 3, col: 3 }).is_err());
    assert!(TicTacToe::apply(&mut state, 0, &Action { row: 1, col: 1 }).is_err());
    assert!(TicTacToe::apply(&mut state, 0, &Action { row: 255, col: 255 }).is_err());
    assert_eq!(state, before);
    TicTacToe::apply(&mut state, 0, &Action { row: 1, col: 3 })?;
    assert_eq!(TicTacToe::returns(&state), vec![1.0, -1.0]);
    assert!(TicTacToe::current_players(&state).is_empty());
    assert!(TicTacToe::legal_actions(&state, 1).is_empty());
    assert!(TicTacToe::apply(&mut state, 1, &Action { row: 3, col: 3 }).is_err());
    Ok(())
}

/// The known reachable-state count checks validation against exhaustive legal play.
#[test]
fn all_reachable_boards_round_trip() -> Result<(), Box<dyn std::error::Error>> {
    fn visit(state: State, seen: &mut std::collections::BTreeSet<String>) -> Result<(), GameError> {
        let notation = TicTacToe::state_to_notation(&state)?;
        if !seen.insert(notation.clone()) {
            return Ok(());
        }
        assert_eq!(TicTacToe::state_from_notation(&Config {}, &notation)?, state);
        for action in TicTacToe::legal_actions(&state, state.to_move) {
            let mut next = state.clone();
            TicTacToe::apply(&mut next, state.to_move, &action)?;
            visit(next, seen)?;
        }
        Ok(())
    }
    let mut seen = std::collections::BTreeSet::new();
    visit(TicTacToe::new_initial_state(&Config {}, 0)?, &mut seen)?;
    assert_eq!(seen.len(), 5478);
    Ok(())
}
