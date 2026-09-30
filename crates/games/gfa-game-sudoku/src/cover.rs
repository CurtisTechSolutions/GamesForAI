use crate::{grid::boxes, Grid, Solutions};

#[derive(Clone, Copy, Default)]
struct Node {
    left: usize,
    right: usize,
    up: usize,
    down: usize,
    column: usize,
    assignment: usize,
}

struct Cover {
    nodes: Vec<Node>,
    sizes: Vec<usize>,
    selected: Vec<usize>,
    count: u8,
    first: Option<Vec<usize>>,
}

impl Cover {
    fn new(columns: usize) -> Self {
        let mut nodes = Vec::with_capacity(columns + 1 + 2916);
        for index in 0..=columns {
            nodes.push(Node {
                left: if index == 0 { columns } else { index - 1 },
                right: if index == columns { 0 } else { index + 1 },
                up: index, down: index, column: index, assignment: 0,
            });
        }
        Self { nodes, sizes: vec![0; columns + 1], selected: Vec::new(), count: 0, first: None }
    }

    fn add(&mut self, columns: [usize; 4], assignment: usize) {
        let start = self.nodes.len();
        for (offset, column) in columns.into_iter().enumerate() {
            let index = self.nodes.len();
            let up = self.nodes[column].up;
            self.nodes.push(Node {
                left: start + (offset + 3) % 4,
                right: start + (offset + 1) % 4,
                up, down: column, column, assignment,
            });
            self.nodes[up].down = index;
            self.nodes[column].up = index;
            self.sizes[column] += 1;
        }
    }

    fn cover(&mut self, column: usize) {
        let node = self.nodes[column];
        self.nodes[node.left].right = node.right;
        self.nodes[node.right].left = node.left;
        let mut row = node.down;
        while row != column {
            let mut next = self.nodes[row].right;
            while next != row {
                let value = self.nodes[next];
                self.nodes[value.up].down = value.down;
                self.nodes[value.down].up = value.up;
                self.sizes[value.column] -= 1;
                next = value.right;
            }
            row = self.nodes[row].down;
        }
    }

    fn uncover(&mut self, column: usize) {
        let mut row = self.nodes[column].up;
        while row != column {
            let mut next = self.nodes[row].left;
            while next != row {
                let value = self.nodes[next];
                self.sizes[value.column] += 1;
                self.nodes[value.up].down = next;
                self.nodes[value.down].up = next;
                next = value.left;
            }
            row = self.nodes[row].up;
        }
        let node = self.nodes[column];
        self.nodes[node.left].right = column;
        self.nodes[node.right].left = column;
    }

    fn search(&mut self) {
        if self.count == 2 { return; }
        if self.nodes[0].right == 0 {
            self.count += 1;
            if self.first.is_none() { self.first = Some(self.selected.clone()); }
            return;
        }
        let mut column = self.nodes[0].right;
        let mut next = self.nodes[column].right;
        while next != 0 {
            if self.sizes[next] < self.sizes[column] { column = next; }
            next = self.nodes[next].right;
        }
        if self.sizes[column] == 0 { return; }
        self.cover(column);
        let mut row = self.nodes[column].down;
        while row != column && self.count < 2 {
            self.selected.push(self.nodes[row].assignment);
            let mut next = self.nodes[row].right;
            while next != row {
                self.cover(self.nodes[next].column);
                next = self.nodes[next].right;
            }
            self.search();
            let mut next = self.nodes[row].left;
            while next != row {
                self.uncover(self.nodes[next].column);
                next = self.nodes[next].left;
            }
            self.selected.pop();
            row = self.nodes[row].down;
        }
        self.uncover(column);
    }
}

pub(crate) fn solve(grid: &Grid) -> Solutions {
    let n = usize::from(grid.size);
    let (height, width) = match boxes(grid.size) {
        Ok(value) => value,
        Err(_) => return Solutions { count: 0, first: None },
    };
    let mut cover = Cover::new(4 * n * n);
    for (index, &given) in grid.cells.iter().enumerate() {
        let row = index / n;
        let col = index % n;
        let region = row / height * (n / width) + col / width;
        let candidates = if given == 0 { grid.candidates(index) } else { 1 << (given - 1) };
        for digit in 0..n {
            if candidates & (1 << digit) == 0 { continue; }
            cover.add([
                1 + index,
                1 + n * n + row * n + digit,
                1 + 2 * n * n + col * n + digit,
                1 + 3 * n * n + region * n + digit,
            ], index * n + digit);
        }
    }
    cover.search();
    let first = cover.first.map(|assignments| {
        let mut cells = vec![0; n * n];
        for assignment in assignments {
            cells[assignment / n] = (assignment % n + 1) as u8;
        }
        Grid { size: grid.size, cells }
    });
    Solutions { count: cover.count, first }
}
