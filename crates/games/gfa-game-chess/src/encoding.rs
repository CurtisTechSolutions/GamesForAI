use crate::Action;

pub(crate) const MOVE_SPACE: u32 = 64 * 73;
pub(crate) const ACTION_SPACE: u32 = 2 * MOVE_SPACE + 1;
const RAYS: [(i8, i8); 8] = [(0,1),(1,1),(1,0),(1,-1),(0,-1),(-1,-1),(-1,0),(-1,1)];
const KNIGHTS: [(i8, i8); 8] = [(1,2),(2,1),(2,-1),(1,-2),(-1,-2),(-2,-1),(-2,1),(-1,2)];

// 56 ray planes, 8 knight planes, 9 underpromotion planes, a1 origin = 0.
// Queen promotions share their normal ray index; the position disambiguates.
pub(crate) fn move_index(text: &str) -> Option<u32> {
    let b = text.as_bytes();
    if !(b.len() == 4 || b.len() == 5)
        || ![b[0],b[2]].iter().all(|x| (b'a'..=b'h').contains(x))
        || ![b[1],b[3]].iter().all(|x| (b'1'..=b'8').contains(x)) {
        return None;
    }
    let dx = b[2] as i8 - b[0] as i8;
    let dy = b[3] as i8 - b[1] as i8;
    let origin = u32::from(b[1] - b'1') * 8 + u32::from(b[0] - b'a');
    let plane = if b.len() == 5 {
        if !((b[1] == b'7' && b[3] == b'8') || (b[1] == b'2' && b[3] == b'1')) || dx.abs() > 1 { return None; }
        let promotion = match b[4] { b'n' => 0, b'b' => 1, b'r' => 2, b'q' => 3, _ => return None };
        if promotion < 3 {
            return Some(origin * 73 + 64 + promotion * 3 + (dx + 1) as u32);
        }
        RAYS.iter().position(|&(x,y)| x == dx && y == dy)? as u32 * 7
    } else if let Some(i) = KNIGHTS.iter().position(|&delta| delta == (dx,dy)) {
        56 + i as u32
    } else {
        let distance = dx.abs().max(dy.abs());
        if distance == 0 || (dx != 0 && dy != 0 && dx.abs() != dy.abs()) { return None; }
        RAYS.iter().position(|&delta| delta == (dx.signum(),dy.signum()))? as u32 * 7 + distance as u32 - 1
    };
    Some(origin * 73 + plane)
}

pub(crate) fn index(action: &Action) -> u32 {
    match action {
        Action::Move { uci } => move_index(uci).unwrap_or(u32::MAX),
        Action::ClaimDraw { intended: None } => MOVE_SPACE,
        Action::ClaimDraw { intended: Some(uci) } => move_index(uci).map(|i| MOVE_SPACE + 1 + i).unwrap_or(u32::MAX),
    }
}
