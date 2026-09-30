use serde::{Deserialize, Serialize};

/// Version-stable SplitMix64 state for reproducible games, not for cryptography.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeededRng {
    state: u64,
}

impl SeededRng {
    /// Start the stream at a caller-provided seed.
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// Consume a single deterministic draw.
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^ (z >> 31)
    }

    /// Uniform bounded draw using rejection sampling; None for an empty range.
    pub fn index(&mut self, upper: usize) -> Option<usize> {
        if upper == 0 {
            return None;
        }
        let bound = upper as u64;
        let threshold = bound.wrapping_neg() % bound;
        loop {
            let sample = self.next_u64();
            if sample >= threshold {
                return Some((sample % bound) as usize);
            }
        }
    }

    /// Deterministic Fisher-Yates shuffle.
    pub fn shuffle<T>(&mut self, values: &mut [T]) {
        for i in (1..values.len()).rev() {
            if let Some(j) = self.index(i + 1) {
                values.swap(i, j);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_is_version_stable() {
        let mut rng = SeededRng::new(0);
        assert_eq!(rng.next_u64(), 16294208416658607535);
        assert_eq!(rng.next_u64(), 7960286522194355700);
        assert_eq!(rng.index(0), None);
    }

    #[test]
    fn serialization_continues_the_same_stream() -> Result<(), Box<dyn std::error::Error>> {
        let mut rng = SeededRng::new(42);
        rng.next_u64();
        let mut restored: SeededRng = serde_json::from_str(&serde_json::to_string(&rng)?)?;
        assert_eq!(rng.next_u64(), restored.next_u64());
        Ok(())
    }
}
