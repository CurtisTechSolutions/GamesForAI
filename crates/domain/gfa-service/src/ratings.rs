//! Glicko-2 rating periods, following Mark Glickman's revised 2012 procedure.
//! https://www.glicko.net/glicko/glicko2.pdf
//! All results in a period use the same pre-period opponent ratings.
use serde::{Deserialize, Serialize};

const SCALE: f64 = 173.7178;
const EPSILON: f64 = 0.000001;
const MAX_ITERATIONS: usize = 128;

/// A rating on the original Glicko scale, with uncertainty and volatility.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rating {
    /// Estimated playing strength; default 1500.
    pub rating: f64,
    /// Rating deviation; default 350. Zero is allowed for a fixed anchor only.
    pub rd: f64,
    /// Volatility on the Glicko-2 scale; default 0.06.
    pub volatility: f64,
    /// Number of rated games included so far.
    pub games_played: u64,
}
impl Default for Rating {
    fn default() -> Self {
        Self {
            rating: 1500.0,
            rd: 350.0,
            volatility: 0.06,
            games_played: 0,
        }
    }
}

/// One outcome against a pre-period opponent rating.
#[derive(Clone, Copy, Debug)]
pub struct RatedResult {
    /// Opponent's rating before any results in this period are applied.
    pub opponent: Rating,
    /// 1 for a win, 0.5 for a draw, 0 for a loss.
    pub score: f64,
}

/// Invalid numeric input or a bounded numerical solver failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RatingError {
    /// Nonfinite/out-of-domain rating, score, or period configuration.
    #[error("invalid Glicko-2 rating period input")]
    InvalidInput,
    /// A period would overflow the played-game counter.
    #[error("rating game counter overflow")]
    CounterOverflow,
    /// The volatility solve did not converge within its iteration limit.
    #[error("Glicko-2 volatility did not converge")]
    NonConvergence,
}

fn valid_score(score: f64) -> bool {
    score == 0.0 || score == 0.5 || score == 1.0
}

impl Rating {
    /// Validate supported numeric bounds without changing a value.
    pub fn validate(self) -> Result<Self, RatingError> {
        if !self.rating.is_finite()
            || !(-10_000.0..=10_000.0).contains(&self.rating)
            || !self.rd.is_finite()
            || !(0.0..=10_000.0).contains(&self.rd)
            || !self.volatility.is_finite()
            || !(0.000001..=10.0).contains(&self.volatility)
        {
            return Err(RatingError::InvalidInput);
        }
        Ok(self)
    }

    /// Approximate 95% interval on the original scale (rating plus/minus two RD).
    pub fn interval95(self) -> Result<[f64; 2], RatingError> {
        self.validate()?;
        Ok([self.rating - 2.0 * self.rd, self.rating + 2.0 * self.rd])
    }

    /// Update one rating period. An empty period increases uncertainty only.
    /// tau is the volatility constraint, 0.2..=1.2; use 0.5 unless calibrated.
    pub fn update_period(self, results: &[RatedResult], tau: f64) -> Result<Self, RatingError> {
        self.validate()?;
        if self.rd == 0.0
            || !tau.is_finite()
            || !(0.2..=1.2).contains(&tau)
            || results.len() > 1_000_000
        {
            return Err(RatingError::InvalidInput);
        }
        let count = u64::try_from(results.len()).map_err(|_| RatingError::CounterOverflow)?;
        let games_played = self
            .games_played
            .checked_add(count)
            .ok_or(RatingError::CounterOverflow)?;
        let mu = (self.rating - 1500.0) / SCALE;
        let phi = self.rd / SCALE;
        if results.is_empty() {
            return Self {
                rd: phi.hypot(self.volatility) * SCALE,
                ..self
            }
            .validate();
        }
        let mut information = 0.0;
        let mut improvement = 0.0;
        for result in results {
            result.opponent.validate()?;
            if !valid_score(result.score) {
                return Err(RatingError::InvalidInput);
            }
            let opponent_mu = (result.opponent.rating - 1500.0) / SCALE;
            let opponent_phi = result.opponent.rd / SCALE;
            let g = 1.0 / (1.0 + 3.0 * opponent_phi.powi(2) / std::f64::consts::PI.powi(2)).sqrt();
            let x = g * (mu - opponent_mu);
            // Computing p*(1-p) from the exponential avoids cancellation when p rounds to 1.
            let z = (-x.abs()).exp();
            let (p, q) = if x >= 0.0 {
                (1.0 / (1.0 + z), z / (1.0 + z))
            } else {
                (z / (1.0 + z), 1.0 / (1.0 + z))
            };
            information += g * g * z / (1.0 + z).powi(2);
            let residual = if result.score == 1.0 {
                q
            } else {
                result.score - p
            };
            improvement += g * residual;
        }
        if !information.is_finite() || information <= 0.0 {
            return Err(RatingError::InvalidInput);
        }
        let variance = information.recip();
        let delta = variance * improvement;
        let sigma = volatility(phi, self.volatility, variance, delta, tau)?;
        let prior_phi = phi.hypot(sigma);
        let next_phi = (prior_phi.powi(-2) + information).sqrt().recip();
        Self {
            rating: SCALE * (mu + next_phi.powi(2) * improvement) + 1500.0,
            rd: SCALE * next_phi,
            volatility: sigma,
            games_played,
        }
        .validate()
    }
}

fn volatility(
    phi: f64,
    sigma: f64,
    variance: f64,
    delta: f64,
    tau: f64,
) -> Result<f64, RatingError> {
    let a = (sigma * sigma).ln();
    let f = |x: f64| {
        let ex = x.exp();
        ex * (delta * delta - phi * phi - variance - ex)
            / (2.0 * (phi * phi + variance + ex).powi(2))
            - (x - a) / (tau * tau)
    };
    let mut left = a;
    let mut right = if delta * delta > phi * phi + variance {
        (delta * delta - phi * phi - variance).ln()
    } else {
        let mut found = None;
        for k in 1..=MAX_ITERATIONS {
            let candidate = a - k as f64 * tau;
            if f(candidate) >= 0.0 {
                found = Some(candidate);
                break;
            }
        }
        found.ok_or(RatingError::NonConvergence)?
    };
    let mut fa = f(left);
    let mut fb = f(right);
    for _ in 0..MAX_ITERATIONS {
        if (right - left).abs() <= EPSILON {
            let result = (left / 2.0).exp();
            return result
                .is_finite()
                .then_some(result)
                .ok_or(RatingError::NonConvergence);
        }
        let denominator = fb - fa;
        if !denominator.is_finite() || denominator == 0.0 {
            return Err(RatingError::NonConvergence);
        }
        let middle = left + (left - right) * fa / denominator;
        let fm = f(middle);
        if !fm.is_finite() {
            return Err(RatingError::NonConvergence);
        }
        if fm * fb <= 0.0 {
            left = right;
            fa = fb;
        } else {
            fa /= 2.0;
        }
        right = middle;
        fb = fm;
    }
    Err(RatingError::NonConvergence)
}

/// Update both sides from their original ratings, independent of update order.
/// Fixed calibrated anchors retain strength, RD and volatility but count games.
pub fn rate_pair(
    left: Rating,
    right: Rating,
    left_score: f64,
    fixed_left: bool,
    fixed_right: bool,
    tau: f64,
) -> Result<[Rating; 2], RatingError> {
    left.validate()?;
    right.validate()?;
    if !valid_score(left_score) || !tau.is_finite() || !(0.2..=1.2).contains(&tau) {
        return Err(RatingError::InvalidInput);
    }
    let update = |rating: Rating, opponent, score, fixed| {
        if fixed {
            Ok(Rating {
                games_played: rating
                    .games_played
                    .checked_add(1)
                    .ok_or(RatingError::CounterOverflow)?,
                ..rating
            })
        } else {
            rating.update_period(&[RatedResult { opponent, score }], tau)
        }
    };
    Ok([
        update(left, right, left_score, fixed_left)?,
        update(right, left, 1.0 - left_score, fixed_right)?,
    ])
}

#[cfg(test)]
mod tests;
