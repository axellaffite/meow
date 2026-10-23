//! The cat held by the scruff: seen from the front, hanging limp, swinging
//! like a pendulum from the pinch point.

use super::{Animal, CANVAS_H, Grid};

/// Canvas pixel (column, row) the cat hangs from, the pinch at the top of the tuft.
pub const SCRUFF: (usize, usize) = (12, HANG_TOP);

const HANG_X: usize = 5;
const HANG_TOP: usize = 1;

const HEAD_AND_BODY: [&str; 15] = [
    "......kk",
    "...k.kook.k",
    "..kpkkookkpk",
    concat!("..k", "oso", "mm", "osok"),
    concat!("..k", "ooo", "mm", "ooo", "k"),
    concat!("..k", "oeo", "mm", "ofo", "k"),
    concat!("..k", "oec", "pp", "cfo", "k"),
    concat!("..k", "o", "wwwwww", "o", "k"),
    concat!("...kk", "wwww", "kk"),
    concat!("...ko", "wwww", "ok"),
    concat!("..koo", "wwww", "ook"),
    concat!("..kko", "wwww", "okk"),
    concat!("...ko", "wwww", "ok"),
    concat!("...koo", "ww", "ook"),
    concat!("...k", "oooooo", "k"),
];

/// Left edge (outline column) of each hind leg.
const LEG_XS: [usize; 2] = [3, 7];
const LEG_LEN: usize = 4;
const TAIL_X: usize = 10;
const TAIL_LEN: usize = 6;

/// Which hind leg is pulled up mid-kick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kick {
    None,
    Left,
    Right,
}

/// Draws the hanging cat; `swing` is the sideways offset of its lowest row, in pixels.
pub(super) fn draw_hanging(grid: &mut Grid, swing: i8, kick: Kick) {
    let mut cat = grid.blank();
    let head_and_body: &[&str] = match grid.animal {
        Animal::Cat => &HEAD_AND_BODY,
        Animal::Pig => &super::pig::HANGING,
        Animal::Corgi => &super::corgi::HANGING,
    };
    cat.blit(head_and_body, HANG_X, HANG_TOP);
    let legs_top = HANG_TOP + head_and_body.len();
    draw_tail(&mut cat, legs_top - 1);
    for (side, &x) in [Kick::Left, Kick::Right].iter().zip(&LEG_XS) {
        let len = if *side == kick { LEG_LEN - 1 } else { LEG_LEN };
        draw_leg(&mut cat, HANG_X + x, legs_top, len);
    }

    // Rows lower down swing further, the scruff itself stays put.
    let span = (CANVAS_H - 1 - HANG_TOP) as f64;
    for y in HANG_TOP..CANVAS_H {
        let shift = (f64::from(swing) * (y - HANG_TOP) as f64 / span).round() as isize;
        for x in 0..super::CANVAS_W {
            let c = cat.cells[y * super::CANVAS_W + x];
            if c != '.' {
                grid.set(x as isize + shift, y, c);
            }
        }
    }
}

fn draw_tail(grid: &mut Grid, top: usize) {
    let x = (HANG_X + TAIL_X) as isize;
    for y in top..top + TAIL_LEN {
        grid.set(x, y, 'k');
        grid.set(x + 1, y, 't');
        grid.set(x + 2, y, 'k');
    }
    grid.set(x + 1, top + TAIL_LEN, 'k');
}

fn draw_leg(grid: &mut Grid, x: usize, top: usize, len: usize) {
    let x = x as isize;
    for y in top..top + len {
        grid.set(x, y, 'k');
        grid.set(x + 1, y, 'l');
        grid.set(x + 2, y, 'l');
        grid.set(x + 3, y, 'k');
    }
    for dx in 0..4 {
        grid.set(x + dx, top + len, 'k');
    }
}
