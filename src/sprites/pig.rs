//! Dédé the pig: round, on the cat's canvas so every pose works on him, with
//! his own legs, his front when held up, and him squatting to poop.

use super::{BUBBLE_H, Grid, Rgb};

/// Head, body and tail, facing right, down to the tops of the legs: a round
/// pig on the cat's canvas, pale belly, ears up front, snout out past the
/// cat's jaw, mud on the flank.
const BODY: [&str; 12] = [
    ".............kok.kok....",
    ".............kockkock...",
    ".tt.........koooooook...",
    "t..t.kkkkkkkkoooooook...",
    "t.tkkoooooooooooowkookk.",
    ".tkoooooooooooooookoccck",
    ".kooossoooooooooooooccnk",
    ".koooooooooooooooqooccck",
    ".kooooooosssooooooookkk.",
    "..kbbbbbbbbbbboooook....",
    "..kllbbbbbbbbbbbllk.....",
    "..kllkllkkkkkllkllk.....",
];

/// Rows of `BODY` that hold the eye, and the same rows with it shut.
const EYE_ROW: usize = 4;
const EYES_CLOSED: [&str; 2] = ["t.tkkooooooooooooooookk.", ".tkooooooooooooookkoccck"];

/// Columns of the legs (the near-side one of each two-pixel leg).
const LEG_XS: [usize; 4] = [3, 6, 13, 16];
/// The row of the hooves, below the legs drawn in `BODY`.
pub(super) const HOOF_ROW: usize = BODY.len();

/// Where his head starts, and how many rows it spans, when he lowers it to eat.
pub(super) const HEAD_X: usize = 12;
pub(super) const HEAD_ROWS: usize = 9;
/// Outline column on the near side of the front leg he lifts to swat.
pub(super) const SWAT_LEG_X: isize = 15;

/// Squatting to poop, eyes shut, tail up: the rump sunk one row, then two.
/// The legs under it stay put, so it hides the hind ones.
const SQUATS: [[&str; 14]; 2] = [
    [
        ".............kok.kok....",
        ".tt..........kockkock...",
        "t..t........koooooook...",
        "t.t........kkoooooook...",
        ".t...kkkkkkooooooooookk.",
        "...kkooooooooooookkoccck",
        "..koooooooooooooooooccnk",
        ".kooossooooooooooqooccck",
        ".kooooooooosooooooookkk.",
        ".kooooooossbbboooook....",
        "..kbbbbbbbbbbbbbllk.....",
        "..kllbbbbbbkkllkllk.....",
        "...khhhkkkkkhhk.khhk....",
        "....kkk.....kk...kk.....",
    ],
    [
        ".............kok.kok....",
        ".............kockkock...",
        ".tt.........koooooook...",
        "t..t.......kkoooooook...",
        "t.t....kkkkooooooooookk.",
        ".t...kkooooooooookkoccck",
        "...kkoooooooooooooooccnk",
        "..koooooooooooooqooccck.",
        ".kooossoooosooooooookkk.",
        ".kooooooossbbboooook....",
        ".kooooobbbbbbbbbllk.....",
        "..kbbbbbbbbkkllkllk.....",
        "..kllbbkkkkkhhk.khhk....",
        "...kkkk.....kk...kk.....",
    ],
];

/// Seen from the front, held up by the skin of his neck: floppy ears, a
/// big snout with two nostrils, a round belly.
pub(super) const HANGING: [&str; 15] = [
    "......kk",
    "..kk.kook.kk",
    ".kcckkookkcck",
    "..kooooooook",
    "..koeoooofok",
    "..koccccccok",
    "..kocnccncok",
    "..koccccccok",
    "...kkooookk",
    "...koooooook",
    "..koobbbbook",
    "..kkobbbbokk",
    "...kobbssok",
    "...kobbbbok",
    "...koooooook",
];

/// How deep the rump sinks for each phase: settling, pushing, letting go.
const SQUAT_DEPTHS: [usize; 3] = [0, 1, 1];

fn body(eyes_closed: bool) -> Vec<&'static str> {
    let mut rows = BODY.to_vec();
    if eyes_closed {
        rows[EYE_ROW..EYE_ROW + 2].copy_from_slice(&EYES_CLOSED);
    }
    rows
}

/// His body without the legs below the belly, for lying down.
pub(super) fn rows(eyes_closed: bool) -> Vec<&'static str> {
    let mut rows = body(eyes_closed);
    rows.truncate(super::LEG_ROW);
    rows
}

/// Standing on his four legs, each hoof shifted by its stride.
pub(super) fn draw_standing(grid: &mut Grid, eyes_closed: bool, strides: [isize; 4]) {
    grid.blit(&body(eyes_closed), 0, BUBBLE_H);
    let y = BUBBLE_H + HOOF_ROW;
    // Outlines first so neighbouring hooves never cover each other.
    for (&x, &dx) in LEG_XS.iter().zip(&strides) {
        let x = x as isize + dx;
        grid.set(x - 1, y, 'k');
        grid.set(x + 2, y, 'k');
        grid.set(x, y + 1, 'k');
        grid.set(x + 1, y + 1, 'k');
    }
    for (&x, &dx) in LEG_XS.iter().zip(&strides) {
        let x = x as isize + dx;
        grid.set(x, y, 'h');
        grid.set(x + 1, y, 'h');
    }
}

/// Squatting, rump sunk toward the floor, tail up.
pub(super) fn draw_squatting(grid: &mut Grid, phase: u8) {
    let depth = SQUAT_DEPTHS[usize::from(phase).min(SQUAT_DEPTHS.len() - 1)];
    grid.blit(&SQUATS[depth], 0, BUBBLE_H);
}

/// A poop, most of it left first.
const POOPS: [&[&str]; 3] = [
    &["...kuvuk...", "..kkkkkkk.."],
    &["....kkk....", "...kuvuk...", "..kuuuvuk..", "..kkkkkkk.."],
    &[
        ".....kk....",
        "....kvuk...",
        "...kuuuuk..",
        "..kvuuuvuk.",
        ".kuuuvuuuuk",
        ".kkkkkkkkkk",
    ],
];

/// Rows of the poop with `left` of it (1 to 3).
pub(super) fn poop_rows(left: u8) -> &'static [&'static str] {
    POOPS[usize::from(left.clamp(1, 3)) - 1]
}

pub(super) fn color(c: char) -> Option<Rgb> {
    match c {
        'u' => Some([122, 80, 44]),
        'v' => Some([164, 112, 62]),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        BUBBLE_H, Bubble, CANVAS_W, Coat, Facing, Frame, LEG_ROW, Look, MOUTH, PROP_H, PROP_W,
        Pose, Prop, render, render_prop,
    };
    use super::*;

    fn pose(pose: Pose) -> Frame {
        let look = Look {
            pose,
            bubble: Bubble::None,
            facing: Facing::Right,
        };
        render(&look, Coat::Dede)
    }

    fn top_row(frame: &Frame, xs: std::ops::Range<usize>) -> usize {
        (0..frame.height)
            .find(|&y| xs.clone().any(|x| frame.get(x, y).is_some()))
            .unwrap()
    }

    #[test]
    fn pig_art_fits_and_uses_known_colors() {
        let all = body(false)
            .into_iter()
            .chain(body(true))
            .chain(SQUATS.iter().flatten().copied())
            .chain(HANGING.iter().copied());
        for row in all {
            assert!(row.len() <= CANVAS_W, "{row}");
            assert!(
                row.chars()
                    .all(|c| c == '.' || Coat::Dede.color(c).is_some()),
                "{row}"
            );
        }
        for row in POOPS.iter().copied().flatten() {
            assert_eq!(row.len(), PROP_W, "{row}");
            assert!(
                row.chars()
                    .all(|c| c == '.' || c == 'k' || color(c).is_some())
            );
        }
        assert!(POOPS.iter().all(|rows| rows.len() <= PROP_H));
    }

    #[test]
    fn the_pig_is_not_the_cat() {
        let stand = Look {
            pose: Pose::Stand,
            bubble: Bubble::None,
            facing: Facing::Right,
        };
        let shape = |frame: &Frame| frame.pixels.iter().map(Option::is_some).collect::<Vec<_>>();
        assert_ne!(
            shape(&render(&stand, Coat::Dede)),
            shape(&render(&stand, Coat::Ginger))
        );
    }

    #[test]
    fn the_pig_carries_things_in_his_snout() {
        let (x, y) = MOUTH;
        for phase in 0..4 {
            let frame = pose(Pose::Walk(phase));
            assert!(frame.get(x, y).is_some(), "his snout");
        }
    }

    #[test]
    fn squatting_sinks_the_rump_not_the_head() {
        let stand = pose(Pose::Stand);
        let squat = pose(Pose::Squat(1));
        let back = 5..7;
        assert_eq!(top_row(&squat, back.clone()), top_row(&stand, back) + 2);
        let head = 13..CANVAS_W;
        assert_eq!(top_row(&squat, head.clone()), top_row(&stand, head));
        let belly = BUBBLE_H + LEG_ROW;
        assert!(squat.get(4, belly + 2).is_some(), "the rump is low");
    }

    #[test]
    fn poop_shrinks_as_it_goes() {
        let painted = |left| {
            render_prop(Prop::Poop(left), Facing::Right)
                .pixels
                .iter()
                .filter(|p| p.is_some())
                .count()
        };
        assert!(painted(3) > painted(2) && painted(2) > painted(1) && painted(1) > 0);
    }
}
