use super::{elimination, Hint, Logic, Technique};

fn combinations(values: &[usize], count: usize) -> Vec<Vec<usize>> {
    let mut groups = Vec::new();
    for (a, &first) in values.iter().enumerate() {
        for (b, &second) in values.iter().enumerate().skip(a + 1) {
            if count == 2 {
                groups.push(vec![first, second]);
            } else {
                for &third in values.iter().skip(b + 1) {
                    groups.push(vec![first, second, third]);
                }
            }
        }
    }
    groups
}

impl Logic {
    pub(super) fn naked(&self, count: usize) -> Option<Hint> {
        let technique = if count == 2 { Technique::NakedPair } else { Technique::NakedTriple };
        for unit in &self.units {
            let cells: Vec<_> = unit.iter().copied().filter(|&i|
                (2..=count as u32).contains(&self.masks[i].count_ones())).collect();
            for group in combinations(&cells, count) {
                let digits = group.iter().fold(0, |mask, &i| mask | self.masks[i]);
                if digits.count_ones() as usize != count { continue; }
                if let Some(step) = elimination(technique, &self.masks,
                    unit.iter().copied().filter(|i| !group.contains(i)), digits) {
                    return Some(step);
                }
            }
        }
        None
    }

    pub(super) fn hidden(&self, count: usize) -> Option<Hint> {
        let technique = if count == 2 { Technique::HiddenPair } else { Technique::HiddenTriple };
        for unit in &self.units {
            let digits: Vec<_> = (0..usize::from(self.grid.size)).filter(|&digit| {
                let cells = unit.iter().filter(|&&i| self.masks[i] & (1 << digit) != 0).count();
                (2..=count).contains(&cells)
            }).collect();
            for group in combinations(&digits, count) {
                let mask = group.iter().fold(0, |mask, &digit| mask | (1 << digit));
                let cells: Vec<_> = unit.iter().copied().filter(|&i| self.masks[i] & mask != 0).collect();
                if cells.len() != count { continue; }
                if let Some(step) = elimination(technique, &self.masks, cells.into_iter(), !mask) {
                    return Some(step);
                }
            }
        }
        None
    }

    pub(super) fn locked(&self) -> Option<Hint> {
        let n = usize::from(self.grid.size);
        for (a, unit) in self.units.iter().enumerate() {
            for digit in 0..n {
                let mask = 1 << digit;
                let cells: Vec<_> = unit.iter().copied().filter(|&i| self.masks[i] & mask != 0).collect();
                if cells.len() < 2 { continue; }
                for (b, target) in self.units.iter().enumerate() {
                    if (a < 2 * n) == (b < 2 * n) || !cells.iter().all(|i| target.contains(i)) {
                        continue;
                    }
                    let technique = if a >= 2 * n { Technique::Pointing } else { Technique::Claiming };
                    if let Some(step) = elimination(technique, &self.masks,
                        target.iter().copied().filter(|i| !unit.contains(i)), mask) {
                        return Some(step);
                    }
                }
            }
        }
        None
    }

    pub(super) fn x_wing(&self) -> Option<Hint> {
        let n = usize::from(self.grid.size);
        for transpose in [false, true] {
            let index = |a, b| if transpose { b * n + a } else { a * n + b };
            for digit in 0..n {
                let bit = 1 << digit;
                let candidates: Vec<Vec<usize>> = (0..n).map(|row|
                    (0..n).filter(|&col| self.masks[index(row, col)] & bit != 0).collect()).collect();
                for first in 0..n {
                    if candidates[first].len() != 2 { continue; }
                    for second in first + 1..n {
                        if candidates[first] != candidates[second] { continue; }
                        let cells = (0..n).filter(|&row| row != first && row != second)
                            .flat_map(|row| candidates[first].iter().map(move |&col| index(row, col)));
                        if let Some(step) = elimination(Technique::XWing, &self.masks, cells, bit) {
                            return Some(step);
                        }
                    }
                }
            }
        }
        None
    }
}
