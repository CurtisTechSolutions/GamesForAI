use gfa_game_sudoku::{generate, solve};
use std::{error::Error, hint::black_box, time::Instant};

fn main() -> Result<(), Box<dyn Error>> {
    let puzzles = (0..32)
        .map(|seed| generate(9, seed))
        .collect::<Result<Vec<_>, _>>()?;
    let mut times = Vec::new();
    for _ in 0..32 {
        for puzzle in &puzzles {
            let start = Instant::now();
            let solution = black_box(solve(black_box(puzzle)));
            times.push(start.elapsed().as_micros());
            assert_eq!(solution.count, 1);
        }
    }
    times.sort_unstable();
    println!(
        "Sudoku exact-cover solve: median={}us p95={}us samples={}",
        times[times.len() / 2],
        times[times.len() * 95 / 100],
        times.len()
    );
    assert!(
        times[times.len() * 95 / 100] < 1000,
        "p95 exceeds the 1 ms target"
    );
    Ok(())
}
