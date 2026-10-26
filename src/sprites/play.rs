//! Playtime: the eating and swatting poses, and the props drawn beside the
//! cat in their own window (the kibble bowl, the mouse it chases, a hairball).

use super::{
    Animal, BUBBLE_H, CANVAS_H, CANVAS_W, Coat, Facing, Frame, Grid, HEAD_TOP, LEG_ROW, Rgb, corgi,
    hairball, pig,
};

pub const PROP_W: usize = 11;
pub const PROP_H: usize = 6;

/// Where the head starts, and how many rows it spans, in the standing cat.
pub(super) const HEAD_X: usize = 13;
pub(super) const HEAD_ROWS: usize = HEAD_TOP.len() + 2;
/// Rows the head dips into the bowl, between bites.
const BITE_DROPS: [usize; 2] = [3, 4];
/// Outline column on the near side of the front leg that lifts to swat.
const SWAT_LEG_X: isize = 16;
/// Outline column on the near side of the lifted paw, in front of the chest.
const SWAT_PAW_X: isize = 20;
/// Canvas pixel (column, row) at the tip of the paw slapped down in `Swat(1)`,
/// where a caught pointer is held.
pub const SWAT_PAW: (usize, usize) = (SWAT_PAW_X as usize + 2, BUBBLE_H + LEG_ROW + 2);

const MOUSE: [&str; 4] = [".....kk....", "...kkpkkk..", "p.kggggkgk.", "pkggggggggp"];
const MOUSE_FEET: [&str; 2] = ["..kk...kk..", "...kk.kk..."];

const BOWL: [&str; 4] = [".kkkkkkkkk.", ".kbbbbbbbk.", "..kbbbbbk..", "...kkkkk..."];
/// Kibble heaped above the rim, fullest first.
const KIBBLES: [[&str; 2]; 3] = [
    ["...........", "...n...d..."],
    ["...........", "..ndn.dnd.."],
    ["....ndn....", "..ndnndnd.."],
];

/// Something next to the cat, drawn in a window of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prop {
    /// A bowl with this much kibble left, 0 (empty) to 3 (full).
    Bowl(u8),
    /// A little mouse running; the phase moves its feet.
    Mouse(u8),
    /// A hairball with this much left to wipe, 1 (a smear) to 3 (all of it).
    Hairball(u8),
    /// Glints where a hairball was wiped clean; the frame twinkles them.
    Sparkle(u8),
    /// Dédé's poop with this much left, 1 (a smear) to 3 (all of it).
    Poop(u8),
}

/// The cat with its head lowered into a bowl in front of it, eyes shut.
pub(super) fn draw_eating(grid: &mut Grid, bite: u8) {
    let mut standing = grid.blank();
    super::draw_standing(&mut standing, true, [0; 4]);
    let drop = BITE_DROPS[bite as usize % BITE_DROPS.len()];
    let (head_x, head_rows) = match grid.animal {
        Animal::Cat => (HEAD_X, HEAD_ROWS),
        Animal::Pig => (pig::HEAD_X, pig::HEAD_ROWS),
        Animal::Corgi => (corgi::HEAD_X, corgi::HEAD_ROWS),
    };
    let is_head = |x: usize, y: usize| x >= head_x && (BUBBLE_H..BUBBLE_H + head_rows).contains(&y);
    let cells = |grid: &Grid| {
        (0..CANVAS_H)
            .flat_map(|y| (0..CANVAS_W).map(move |x| (x, y)))
            .map(|(x, y)| (x, y, grid.cells[y * CANVAS_W + x]))
            .filter(|&(_, _, c)| c != '.')
            .collect::<Vec<_>>()
    };
    let all = cells(&standing);
    for &(x, y, c) in all.iter().filter(|&&(x, y, _)| !is_head(x, y)) {
        grid.set(x as isize, y, c);
    }
    // Leaning in: the head moves down and a pixel forward.
    for &(x, y, c) in all.iter().filter(|&&(x, y, _)| is_head(x, y)) {
        grid.set(x as isize + 1, y + drop, c);
    }
}

/// The standing cat with its front paw lifted off the ground: raised high,
/// or struck forward and down.
pub(super) fn draw_swatting(grid: &mut Grid, phase: u8) {
    super::draw_standing(grid, false, [0; 4]);
    let leg_x = match grid.animal {
        Animal::Cat => SWAT_LEG_X,
        Animal::Pig => pig::SWAT_LEG_X,
        Animal::Corgi => corgi::SWAT_LEG_X,
    };
    let belly = BUBBLE_H + LEG_ROW;
    // Take the front leg off the ground; its near outline is shared with the other leg.
    for y in belly + 1..=belly + 3 {
        for x in leg_x + 1..=leg_x + 3 {
            grid.set(x, y, '.');
        }
    }
    grid.set(leg_x + 1, belly, 'k');
    grid.set(leg_x + 2, belly, 'k');
    // Raised in front of the chin, then slapped down to the ground.
    let (top, len) = if phase.is_multiple_of(2) {
        (belly - 5, 4)
    } else {
        (belly - 1, 3)
    };
    draw_paw(grid, SWAT_PAW_X, top, len);
}

/// A leg `len` rows long hanging from `top`, outlined, with a rounded tip.
fn draw_paw(grid: &mut Grid, x: isize, top: usize, len: usize) {
    grid.set(x + 1, top - 1, 'k');
    grid.set(x + 2, top - 1, 'k');
    for y in top..top + len {
        grid.set(x, y, 'k');
        grid.set(x + 1, y, 'l');
        grid.set(x + 2, y, 'l');
        grid.set(x + 3, y, 'k');
    }
    grid.set(x + 1, top + len, 'k');
    grid.set(x + 2, top + len, 'k');
}

fn prop_rows(prop: Prop) -> Vec<&'static str> {
    match prop {
        Prop::Bowl(level) => {
            let heap: &[&str] = match level.min(3) {
                0 => &["...........", "..........."],
                level => &KIBBLES[level as usize - 1],
            };
            heap.iter().chain(&BOWL).copied().collect()
        }
        Prop::Mouse(phase) => {
            let feet = MOUSE_FEET[phase as usize % MOUSE_FEET.len()];
            MOUSE.iter().copied().chain([feet]).collect()
        }
        Prop::Hairball(dirt) => hairball::hairball_rows(dirt).to_vec(),
        Prop::Sparkle(frame) => hairball::sparkle_rows(frame).to_vec(),
        Prop::Poop(left) => pig::poop_rows(left).to_vec(),
    }
}

fn prop_color(c: char) -> Option<Rgb> {
    match c {
        'g' => Some([166, 160, 172]),
        'b' => Some([86, 140, 206]),
        'n' => Some([176, 112, 56]),
        'd' => Some([122, 72, 36]),
        // Outline and pink as on the cat; props never wear its coat.
        other => hairball::color(other)
            .or_else(|| pig::color(other))
            .or_else(|| Coat::Ginger.color(other)),
    }
}

/// The prop drawn facing right, mirrored for left; sits on the bottom row.
pub fn render_prop(prop: Prop, facing: Facing) -> Frame {
    let rows = prop_rows(prop);
    let offset = PROP_H - rows.len();
    let mut pixels = vec![None; PROP_W * PROP_H];
    for (dy, row) in rows.iter().enumerate() {
        for (dx, c) in row.chars().enumerate() {
            let x = match facing {
                Facing::Right => dx,
                Facing::Left => PROP_W - 1 - dx,
            };
            pixels[(offset + dy) * PROP_W + x] = prop_color(c);
        }
    }
    Frame {
        width: PROP_W,
        height: PROP_H,
        pixels,
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Bubble, Look, Pose, render};
    use super::*;

    fn pose(pose: Pose) -> Frame {
        let look = Look {
            pose,
            bubble: Bubble::None,
            facing: Facing::Right,
        };
        render(&look, Coat::Ginger)
    }

    fn lowest_row(frame: &Frame, xs: std::ops::Range<usize>) -> Option<usize> {
        (0..frame.height)
            .rev()
            .find(|&y| xs.clone().any(|x| frame.get(x, y).is_some()))
    }

    #[test]
    fn prop_art_fits_and_uses_known_colors() {
        let all = MOUSE
            .iter()
            .chain(&MOUSE_FEET)
            .chain(&BOWL)
            .chain(KIBBLES.iter().flatten());
        for row in all {
            assert_eq!(row.len(), PROP_W, "{row}");
            assert!(
                row.chars().all(|c| c == '.' || prop_color(c).is_some()),
                "{row}"
            );
        }
    }

    #[test]
    fn eating_lowers_the_head_and_bobs() {
        let standing = pose(Pose::Stand);
        let eating = pose(Pose::Eat(0));
        let head_top =
            |f: &Frame| (0..f.height).find(|&y| (HEAD_X..CANVAS_W).any(|x| f.get(x, y).is_some()));
        assert!(head_top(&eating) > head_top(&standing));
        assert_ne!(pose(Pose::Eat(0)), pose(Pose::Eat(1)));
    }

    #[test]
    fn swatting_lifts_the_front_paw() {
        let ground = CANVAS_H - 1;
        let front_foot = |f: &Frame| (17..20).any(|x| f.get(x, ground).is_some());
        assert!(front_foot(&pose(Pose::Stand)));
        assert!(!front_foot(&pose(Pose::Swat(0))));
        assert!(!front_foot(&pose(Pose::Swat(1))));
        assert_ne!(pose(Pose::Swat(0)), pose(Pose::Swat(1)));
    }

    #[test]
    fn paw_tip_is_on_the_slapped_down_paw() {
        let (x, y) = SWAT_PAW;
        assert!(pose(Pose::Swat(1)).get(x, y).is_some());
        assert!(pose(Pose::Stand).get(x, y).is_none());
    }

    #[test]
    fn props_sit_on_the_ground() {
        for prop in [
            Prop::Bowl(3),
            Prop::Bowl(0),
            Prop::Mouse(0),
            Prop::Mouse(1),
            Prop::Hairball(3),
            Prop::Hairball(2),
            Prop::Hairball(1),
            Prop::Poop(3),
            Prop::Poop(2),
            Prop::Poop(1),
        ] {
            let frame = render_prop(prop, Facing::Right);
            assert_eq!(lowest_row(&frame, 0..PROP_W), Some(PROP_H - 1), "{prop:?}");
        }
    }

    #[test]
    fn bowl_empties_as_the_cat_eats() {
        let kibble = |level| {
            let frame = render_prop(Prop::Bowl(level), Facing::Right);
            frame
                .pixels
                .iter()
                .filter(|p| **p == prop_color('n') || **p == prop_color('d'))
                .count()
        };
        assert!(kibble(3) > kibble(2) && kibble(2) > kibble(1) && kibble(1) > kibble(0));
        assert_eq!(kibble(0), 0);
    }

    #[test]
    fn mouse_runs_and_mirrors() {
        assert_ne!(
            render_prop(Prop::Mouse(0), Facing::Right),
            render_prop(Prop::Mouse(1), Facing::Right)
        );
        let right = render_prop(Prop::Mouse(0), Facing::Right);
        let left = render_prop(Prop::Mouse(0), Facing::Left);
        for y in 0..PROP_H {
            for x in 0..PROP_W {
                assert_eq!(right.get(x, y), left.get(PROP_W - 1 - x, y));
            }
        }
    }
}
