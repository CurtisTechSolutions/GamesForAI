//! Reproducible single-threaded random playout throughput measurement.
use gfa_core::{Game, SeededRng};
use gfa_game_tictactoe::{Config, TicTacToe};
use std::{hint::black_box, time::Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut rng = SeededRng::new(42);
    let started = Instant::now();
    let mut steps = 0_u64;
    for seed in 0..100_000 {
        let mut state = TicTacToe::new_initial_state(&Config::default(), seed)?;
        while let Some(&player) = TicTacToe::current_players(&state).first() {
            let actions = TicTacToe::legal_actions(&state, player);
            let index = rng.index(actions.len()).ok_or("empty active action set")?;
            TicTacToe::apply(&mut state, player, black_box(&actions[index]))?;
            steps += 1;
        }
        black_box(TicTacToe::returns(&state));
    }
    let elapsed = started.elapsed().as_secs_f64();
    println!("tictactoe random playouts: {steps} steps in {elapsed:.3}s");
    println!("steps_per_second={:.0}", steps as f64 / elapsed);
    Ok(())
}
