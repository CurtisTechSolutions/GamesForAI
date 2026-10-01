use super::*;

#[test]
fn matches_published_glickman_example() -> Result<(), RatingError> {
    let player = Rating { rd: 200.0, ..Rating::default() };
    let results = [(1400.0, 30.0, 1.0), (1550.0, 100.0, 0.0), (1700.0, 300.0, 0.0)]
        .map(|(rating, rd, score)| RatedResult { opponent: Rating { rating, rd, ..Rating::default() }, score });
    let next = player.update_period(&results, 0.5)?;
    assert!((next.rating - 1464.06).abs() < 0.01);
    assert!((next.rd - 151.52).abs() < 0.01);
    assert!((next.volatility - 0.059996).abs() < 0.000001);
    assert_eq!(next.games_played, 3);
    let mut reversed = results;
    reversed.reverse();
    let other = player.update_period(&reversed, 0.5)?;
    assert!((other.rating - next.rating).abs() < 0.0000001);
    Ok(())
}

#[test]
fn inactive_period_and_intervals_preserve_rating_and_volatility() -> Result<(), RatingError> {
    let original = Rating::default();
    let next = original.update_period(&[], 0.5)?;
    assert_eq!(next.rating, original.rating);
    assert_eq!(next.volatility, original.volatility);
    assert_eq!(next.games_played, 0);
    assert!(next.rd > original.rd);
    assert_eq!(original.interval95()?, [800.0, 2200.0]);
    Ok(())
}

#[test]
fn pair_uses_original_ratings_and_fixed_anchors_do_not_drift() -> Result<(), RatingError> {
    let original = Rating::default();
    let [winner, loser] = rate_pair(original, original, 1.0, false, false, 0.5)?;
    assert!(winner.rating > 1500.0);
    assert!(loser.rating < 1500.0);
    assert!((winner.rating + loser.rating - 3000.0).abs() < 0.000001);
    assert_eq!(winner.rd, loser.rd);
    let [drawn, other] = rate_pair(original, original, 0.5, false, false, 0.5)?;
    assert_eq!(drawn, other);
    assert_eq!(drawn.rating, 1500.0);
    assert!(drawn.rd < original.rd);
    let anchor = Rating { rating: 1800.0, rd: 0.0, ..original };
    let [learner, unchanged] = rate_pair(original, anchor, 1.0, false, true, 0.5)?;
    assert!(learner.rating > winner.rating);
    assert_eq!(unchanged, Rating { games_played: 1, ..anchor });
    assert!(anchor.update_period(&[], 0.5).is_err());
    Ok(())
}

#[test]
fn extreme_but_valid_rating_gaps_remain_finite() -> Result<(), RatingError> {
    let high = Rating { rating: 9000.0, rd: 30.0, ..Rating::default() };
    let low = Rating { rating: -9000.0, rd: 30.0, ..Rating::default() };
    for score in [0.0, 0.5, 1.0] {
        for rating in rate_pair(high, low, score, false, false, 0.5)? {
            rating.validate()?;
        }
    }
    Ok(())
}

#[test]
fn invalid_numbers_and_counter_overflow_fail_without_mutation() {
    let original = Rating::default();
    for score in [f64::NAN, f64::INFINITY, -1.0, 0.25, 2.0] {
        assert!(rate_pair(original, original, score, false, false, 0.5).is_err());
    }
    for tau in [0.0, f64::NAN, f64::INFINITY, 2.0] {
        assert!(original.update_period(&[], tau).is_err());
    }
    for invalid in [
        Rating { rd: -1.0, ..original },
        Rating { rating: f64::NAN, ..original },
        Rating { volatility: 0.0, ..original },
        Rating { volatility: f64::INFINITY, ..original },
    ] {
        assert_eq!(invalid.validate(), Err(RatingError::InvalidInput));
    }
    let full = Rating { games_played: u64::MAX, ..original };
    assert_eq!(rate_pair(full, original, 1.0, false, false, 0.5), Err(RatingError::CounterOverflow));
}
