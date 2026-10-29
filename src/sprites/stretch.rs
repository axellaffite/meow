//! Stretching: the bow, front paws reaching ahead with the rump up, and the
//! arched back; and scratching, reared up against the screen's edge.

use super::{Animal, BUBBLE_H, CANVAS_H, CANVAS_W, Grid, LEG_ROW, corgi, pig, play};

/// How far the bow tips the body, rump up and chest down, for each
/// `Pose::Bow` phase: easing into it, then all the way.
const BOW_DEPTHS: [f64; 2] = [1.0, 2.0];
/// How far the front paws reach ahead in the bow, for each phase.
const BOW_REACHES: [isize; 2] = [1, 3];
/// How high the middle of the back rises, for each `Pose::Arch` phase.
const ARCH_HEIGHTS: [f64; 2] = [1.0, 3.0];
/// The columns the arch rises from and falls back to: the back, not the head.
const ARCH_FROM: f64 = 2.0;
const ARCH_TO: f64 = 13.0;
/// Columns from here on hold the front legs.
const FRONT_X: usize = CANVAS_W / 2;
/// The front paws' strides for each `Pose::Scratch` phase, raking the wall
/// in turn while the hind paws hold still.
const SCRATCH_STRIDES: [[isize; 4]; 4] = [[0, 0, -1, 1], [0; 4], [0, 0, 1, -1], [0; 4]];

/// Rump up, chest down, front paws reaching ahead; eyes shut all the way.
pub(super) fn draw_bowing(grid: &mut Grid, phase: u8) {
    let phase = usize::from(phase).min(BOW_DEPTHS.len() - 1);
    let depth = BOW_DEPTHS[phase];
    // From `-depth` at the tail to `depth` at the nose.
    let tip = |x: usize| (depth * (x as f64 - 11.0) / 11.0).round() as isize;
    let head = (0, tip(CANVAS_W - 1));
    bend(grid, phase > 0, tip, head, BOW_REACHES[phase]);
}

/// The middle of the back raised high, legs straight under it, head low.
pub(super) fn draw_arching(grid: &mut Grid, phase: u8) {
    let phase = usize::from(phase).min(ARCH_HEIGHTS.len() - 1);
    let height = ARCH_HEIGHTS[phase];
    let rise = |x: usize| {
        let along = (x as f64 - ARCH_FROM) / (ARCH_TO - ARCH_FROM);
        if !(0.0..=1.0).contains(&along) {
            return 0;
        }
        -(height * (along * std::f64::consts::PI).sin()).round() as isize
    };
    bend(grid, phase > 0, rise, (0, phase as isize), 0);
}

/// Reared up against the wall like `Pose::Cling`, hind paws still, the
/// front ones raking it.
pub(super) fn draw_scratching(grid: &mut Grid, phase: u8) {
    super::draw_clinging(
        grid,
        SCRATCH_STRIDES[usize::from(phase) % SCRATCH_STRIDES.len()],
    );
}

/// The standing animal with each column of its body moved `shift(x)` rows
/// (up when negative), its head moved by `head`, its front paws `reach`
/// columns ahead, and its legs still on the ground: stretched to meet a
/// raised body, covered by a lowered one.
fn bend(
    grid: &mut Grid,
    eyes_closed: bool,
    shift: impl Fn(usize) -> isize,
    head: (isize, isize),
    reach: isize,
) {
    let mut standing = grid.blank();
    super::draw_standing(&mut standing, eyes_closed, [0; 4]);
    let (head_x, head_rows, leg_top) = match grid.animal {
        Animal::Cat => (play::HEAD_X, play::HEAD_ROWS, BUBBLE_H + LEG_ROW + 1),
        Animal::Pig => (pig::HEAD_X, pig::HEAD_ROWS, BUBBLE_H + pig::HOOF_ROW),
        Animal::Corgi => (corgi::HEAD_X, corgi::HEAD_ROWS, BUBBLE_H + corgi::PAW_ROW),
    };
    let is_head = |x: usize, y: usize| x >= head_x && (BUBBLE_H..BUBBLE_H + head_rows).contains(&y);
    let cells: Vec<_> = (0..CANVAS_H)
        .flat_map(|y| (0..CANVAS_W).map(move |x| (x, y)))
        .map(|(x, y)| (x, y, standing.cells[y * CANVAS_W + x]))
        .filter(|&(_, _, c)| c != '.')
        .collect();
    let mut put = |x: isize, y: isize, c: char| {
        if y >= 0 {
            grid.set(x, y as usize, c);
        }
    };
    for &(x, y, c) in cells.iter().filter(|&&(_, y, _)| y >= leg_top) {
        let to_x = x as isize + if x >= FRONT_X { reach } else { 0 };
        put(to_x, y as isize, c);
        // The top of a leg reaches up to a raised body.
        if y == leg_top {
            for up in shift(x)..0 {
                put(to_x, (y as isize) + up, c);
            }
        }
    }
    for &(x, y, c) in cells
        .iter()
        .filter(|&&(x, y, _)| y < leg_top && !is_head(x, y))
    {
        put(x as isize, y as isize + shift(x), c);
    }
    for &(x, y, c) in cells.iter().filter(|&&(x, y, _)| is_head(x, y)) {
        put(x as isize + head.0, y as isize + head.1, c);
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Bubble, Coat, Facing, Frame, Look, Pose, render};
    use super::*;

    fn pose(pose: Pose, coat: Coat) -> Frame {
        let look = Look {
            pose,
            bubble: Bubble::None,
            facing: Facing::Right,
        };
        render(&look, coat)
    }

    /// The top painted row in columns `xs`.
    fn top_row(frame: &Frame, xs: std::ops::RangeInclusive<usize>) -> usize {
        (0..frame.height)
            .find(|&y| xs.clone().any(|x| frame.get(x, y).is_some()))
            .unwrap()
    }

    fn on_the_ground(frame: &Frame) -> bool {
        (0..CANVAS_W).any(|x| frame.get(x, CANVAS_H - 1).is_some())
    }

    #[test]
    fn bowing_raises_the_rump_lowers_the_head_and_reaches_ahead() {
        for coat in Coat::ALL {
            let stand = pose(Pose::Stand, coat);
            let bow = pose(Pose::Bow(1), coat);
            assert!(top_row(&bow, 2..=6) < top_row(&stand, 2..=6), "{coat:?}");
            let head = 14..=CANVAS_W - 1;
            assert!(
                top_row(&bow, head.clone()) > top_row(&stand, head),
                "{coat:?}"
            );
            let ground = |f: &Frame| {
                (0..CANVAS_W)
                    .rev()
                    .find(|&x| f.get(x, CANVAS_H - 1).is_some())
                    .unwrap()
            };
            assert!(ground(&bow) > ground(&stand), "{coat:?} paws ahead");
            assert!(on_the_ground(&bow), "{coat:?}");
            assert_ne!(pose(Pose::Bow(0), coat), bow, "{coat:?} eases into it");
        }
    }

    #[test]
    fn arching_raises_the_middle_of_the_back_on_straight_legs() {
        for coat in Coat::ALL {
            let stand = pose(Pose::Stand, coat);
            let arch = pose(Pose::Arch(1), coat);
            assert!(
                top_row(&arch, 6..=9) + 2 <= top_row(&stand, 6..=9),
                "{coat:?}"
            );
            assert!(on_the_ground(&arch), "{coat:?}");
            assert!(top_row(&pose(Pose::Arch(0), coat), 6..=9) > top_row(&arch, 6..=9));
        }
    }

    #[test]
    fn scratching_rakes_the_wall_with_the_front_paws() {
        let wall = |f: &Frame| {
            (0..CANVAS_H)
                .filter(|&y| f.get(CANVAS_W - 1, y).is_some())
                .collect::<Vec<_>>()
        };
        let a = pose(Pose::Scratch(0), Coat::Ginger);
        let b = pose(Pose::Scratch(2), Coat::Ginger);
        assert!(!wall(&a).is_empty(), "paws on the wall");
        assert_ne!(wall(&a), wall(&b), "the paws move along it");
        assert!(on_the_ground(&a), "reared up from the floor");
    }
}
