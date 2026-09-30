use super::*;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const PUZZLE: &str =
    "530070000600195000098000060800060003400803001700020006060000280000419005000080079";
const SOLUTION: &str =
    "534678912672195348198342567859761423426853791713924856961537284287419635345286179";

#[test]
fn solves_known_grid_and_stops_after_two_solutions() -> TestResult {
    let grid = Grid::parse(9, PUZZLE)?;
    let result = solve(&grid);
    assert_eq!(result.count, 1);
    assert_eq!(result.first.ok_or("missing solution")?.notation(), SOLUTION);
    assert_eq!(solve(&Grid::parse(4, "................")?).count, 2);
    assert_eq!(solve(&Grid::parse(9, SOLUTION)?).count, 1);
    Ok(())
}

#[test]
fn rejects_bad_dimensions_digits_and_conflicts() {
    for (size, input) in [
        (3, "........."),
        (4, "11.............."),
        (4, "1...1..........."),
        (4, "1....1.........."),
        (4, "5..............."),
        (4, "é.............."),
        (4, "..............."),
        (4, "................."),
        (6, "7..................................."),
    ] {
        assert!(Grid::parse(size, input).is_err(), "{size}: {input}");
    }
    assert!(Grid::from_cells(9, vec![0; 80]).is_err());
}

#[test]
fn exact_cover_matches_independent_four_by_four_backtracking() -> TestResult {
    let mut rng = SeededRng::new(581);
    let mut unsatisfiable = 0;
    for _ in 0..2000 {
        let mut cells = vec![0_u8; 16];
        for _ in 0..6 {
            let index = rng.index(16).ok_or("index")?;
            cells[index] = (rng.index(4).ok_or("digit")? + 1) as u8;
        }
        if let Ok(grid) = Grid::from_cells(4, cells.clone()) {
            let expected = brute_force(&mut cells, 0);
            unsatisfiable += usize::from(expected == 0);
            let actual = solve(&grid);
            assert_eq!(actual.count, expected, "{}", grid.notation());
            if let Some(solution) = actual.first {
                assert!(solution.cells().iter().all(|&digit| digit > 0));
                Grid::from_cells(4, solution.cells().to_vec())?;
                for (&given, &digit) in grid.cells().iter().zip(solution.cells()) {
                    assert!(given == 0 || given == digit);
                }
            }
        }
    }
    assert!(
        unsatisfiable > 0,
        "exercise consistent but unsatisfiable grids"
    );
    Ok(())
}

fn brute_force(cells: &mut [u8], index: usize) -> u8 {
    if index == 16 {
        return 1;
    }
    if cells[index] != 0 {
        return brute_force(cells, index + 1);
    }
    let row = index / 4;
    let col = index % 4;
    let mut count = 0;
    for digit in 1..=4 {
        if (0..16).any(|other| {
            cells[other] == digit
                && (other / 4 == row
                    || other % 4 == col
                    || (other / 4 / 2 == row / 2 && other % 4 / 2 == col / 2))
        }) {
            continue;
        }
        cells[index] = digit;
        count += brute_force(cells, index + 1);
        cells[index] = 0;
        if count >= 2 {
            return 2;
        }
    }
    count
}

#[test]
fn generation_is_seeded_unique_and_preserves_rectangular_boxes() -> TestResult {
    for size in [4, 6, 9] {
        let mut distinct = std::collections::BTreeSet::new();
        for seed in 0..24 {
            let grid = generate(size, seed)?;
            assert_eq!(grid, generate(size, seed)?);
            assert_eq!(solve(&grid).count, 1);
            assert!(grid.cells().contains(&0));
            distinct.insert(grid.notation());
            if size == 4 {
                for index in 0..16 {
                    if grid.cells[index] != 0 {
                        let mut fewer = grid.clone();
                        fewer.cells[index] = 0;
                        assert_eq!(solve(&fewer).count, 2, "removable clue");
                    }
                }
            }
        }
        assert!(distinct.len() > 20);
    }
    assert!(generate(5, 0).is_err());
    Ok(())
}
