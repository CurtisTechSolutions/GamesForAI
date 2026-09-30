use gfa_core::GameError;

/// A rule-consistent square grid, in row-major order. Zero denotes an empty cell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grid {
    pub(crate) size: u8,
    pub(crate) cells: Vec<u8>,
}

pub(crate) fn boxes(size: u8) -> Result<(usize, usize), GameError> {
    match size {
        4 => Ok((2, 2)),
        6 => Ok((2, 3)),
        9 => Ok((3, 3)),
        _ => Err(GameError::position("Sudoku size must be 4, 6, or 9")),
    }
}

impl Grid {
    /// Parse exactly size² ASCII digits; '.' and '0' both mean empty.
    pub fn parse(size: u8, notation: &str) -> Result<Self, GameError> {
        boxes(size)?;
        if notation.len() != usize::from(size).pow(2) {
            return Err(GameError::position(
                "Grid must contain exactly size squared cells",
            ));
        }
        let cells = notation
            .bytes()
            .map(|byte| match byte {
                b'.' | b'0' => Ok(0),
                b'1'..=b'9' if byte - b'0' <= size => Ok(byte - b'0'),
                _ => Err(GameError::position(
                    "Use '.' or digits from 1 through the grid size",
                )),
            })
            .collect::<Result<Vec<_>, _>>()?;
        Self::from_cells(size, cells)
    }

    /// Validate dimensions, digit range, and row/column/box consistency.
    pub fn from_cells(size: u8, cells: Vec<u8>) -> Result<Self, GameError> {
        let (height, width) = boxes(size)?;
        let n = usize::from(size);
        if cells.len() != n * n || cells.iter().any(|&digit| digit > size) {
            return Err(GameError::position(
                "Grid dimensions or digit range are invalid",
            ));
        }
        let mut rows = vec![0_u16; n];
        let mut cols = vec![0_u16; n];
        let mut regions = vec![0_u16; n];
        for (index, &digit) in cells.iter().enumerate() {
            if digit == 0 {
                continue;
            }
            let row = index / n;
            let col = index % n;
            let region = row / height * (n / width) + col / width;
            let bit = 1 << (digit - 1);
            if (rows[row] | cols[col] | regions[region]) & bit != 0 {
                return Err(GameError::position(format!(
                    "Digit {digit} at r{}c{} duplicates a row, column, or box",
                    row + 1,
                    col + 1
                )));
            }
            rows[row] |= bit;
            cols[col] |= bit;
            regions[region] |= bit;
        }
        Ok(Self { size, cells })
    }

    /// Width and height in cells.
    pub fn size(&self) -> u8 {
        self.size
    }

    /// Row-major cells, with zero for empty.
    pub fn cells(&self) -> &[u8] {
        &self.cells
    }

    /// Canonical notation, using '.' for empty cells.
    pub fn notation(&self) -> String {
        self.cells
            .iter()
            .map(|&digit| {
                if digit == 0 {
                    '.'
                } else {
                    char::from(b'0' + digit)
                }
            })
            .collect()
    }

    /// Candidate bit mask for a zero-based cell, excluding its current value.
    /// Returns zero for an out-of-range index.
    pub fn candidates(&self, index: usize) -> u16 {
        let n = usize::from(self.size);
        if index >= n * n {
            return 0;
        }
        let (height, width) = match boxes(self.size) {
            Ok(value) => value,
            Err(_) => return 0,
        };
        let row = index / n;
        let col = index % n;
        let mut used = 0_u16;
        for (other, &digit) in self.cells.iter().enumerate() {
            if other == index || digit == 0 {
                continue;
            }
            let r = other / n;
            let c = other % n;
            if r == row || c == col || (r / height == row / height && c / width == col / width) {
                used |= 1 << (digit - 1);
            }
        }
        ((1 << self.size) - 1) & !used
    }
}
