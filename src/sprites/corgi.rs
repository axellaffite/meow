//! The corgi: long and low on the cat's canvas, big upright ears, a white
//! blaze and chest, a fluffy stub of a tail and short white legs.

use super::{BUBBLE_H, Grid};

/// Head, body and tail, facing right, down to the tops of the legs.
const BODY: [&str; 12] = [
    "..............k....k....",
    "............kpk..kpk....",
    "............kppkkppk....",
    "............kooooooook..",
    "..tt........koeommofok..",
    ".ttkkkkkkkkkoooommcccckk",
    ".tkoooooooooooooocccccck",
    "..kooossssooooowwcckkk..",
    "..koooooooooooowwwwk....",
    "..kooooooooooowwwwk.....",
    "..kllwwwwwwwwwwwllk.....",
    "..kllkllkkkkkllkllk.....",
];

/// The row of `BODY` that holds the eyes, and the same row with them shut.
const EYE_ROW: usize = 4;
const EYES_CLOSED: &str = "..tt........kokommokok..";

/// Columns of the legs (the near-side one of each two-pixel leg).
const LEG_XS: [usize; 4] = [3, 6, 13, 16];
/// The row of the paws, below the legs drawn in `BODY`.
const PAW_ROW: usize = BODY.len();

/// Where the head starts, and how many rows it spans, when it lowers it to eat.
pub(super) const HEAD_X: usize = 12;
pub(super) const HEAD_ROWS: usize = 9;
/// Outline column on the near side of the front leg it lifts to swat.
pub(super) const SWAT_LEG_X: isize = 15;

/// Seen from the front, held up by the scruff: big ears, a white blaze down
/// to the muzzle, a white chest.
pub(super) const HANGING: [&str; 15] = [
    "......kk......",
    ".k...kook...k.",
    ".kpkkommokkpk.",
    ".kppoommooppk.",
    "..koeommofok..",
    "..koocmmcook..",
    "...kcckkcck...",
    "....kcccck....",
    "...kkwwwwkk...",
    "..koowwwwook..",
    "..koowwwwook..",
    "..koowwwwook..",
    "..koowwwwook..",
    "...koowwook...",
    "...koooooook..",
];

fn body(eyes_closed: bool) -> Vec<&'static str> {
    let mut rows = BODY.to_vec();
    if eyes_closed {
        rows[EYE_ROW] = EYES_CLOSED;
    }
    rows
}

/// Its body without the legs below the belly, for lying down.
pub(super) fn rows(eyes_closed: bool) -> Vec<&'static str> {
    let mut rows = body(eyes_closed);
    rows.truncate(super::LEG_ROW);
    rows
}

/// Standing on its four short legs, each paw shifted by its stride.
pub(super) fn draw_standing(grid: &mut Grid, eyes_closed: bool, strides: [isize; 4]) {
    grid.blit(&body(eyes_closed), 0, BUBBLE_H);
    let y = BUBBLE_H + PAW_ROW;
    // Outlines first so neighbouring paws never cover each other.
    for (&x, &dx) in LEG_XS.iter().zip(&strides) {
        let x = x as isize + dx;
        grid.set(x - 1, y, 'k');
        grid.set(x + 2, y, 'k');
        grid.set(x, y + 1, 'k');
        grid.set(x + 1, y + 1, 'k');
    }
    for (&x, &dx) in LEG_XS.iter().zip(&strides) {
        let x = x as isize + dx;
        grid.set(x, y, 'l');
        grid.set(x + 1, y, 'l');
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Bubble, CANVAS_W, Coat, Facing, Frame, Look, MOUTH, Pose, render};
    use super::*;

    fn pose(pose: Pose) -> Frame {
        let look = Look {
            pose,
            bubble: Bubble::None,
            facing: Facing::Right,
        };
        render(&look, Coat::Corgi)
    }

    #[test]
    fn corgi_art_fits_and_uses_known_colors() {
        let all = body(false)
            .into_iter()
            .chain(body(true))
            .chain(HANGING.iter().copied());
        for row in all {
            assert!(row.len() <= CANVAS_W, "{row}");
            assert!(
                row.chars()
                    .all(|c| c == '.' || Coat::Corgi.color(c).is_some()),
                "{row}"
            );
        }
    }

    #[test]
    fn the_corgi_is_neither_the_cat_nor_the_pig() {
        let shape = |frame: &Frame| frame.pixels.iter().map(Option::is_some).collect::<Vec<_>>();
        let stand = |coat| {
            let look = Look {
                pose: Pose::Stand,
                bubble: Bubble::None,
                facing: Facing::Right,
            };
            shape(&render(&look, coat))
        };
        assert_ne!(stand(Coat::Corgi), stand(Coat::Ginger));
        assert_ne!(stand(Coat::Corgi), stand(Coat::Dede));
    }

    #[test]
    fn the_corgi_carries_things_in_its_mouth() {
        let (x, y) = MOUTH;
        for phase in 0..4 {
            assert!(pose(Pose::Walk(phase)).get(x, y).is_some());
        }
    }

    #[test]
    fn blinking_shuts_the_eyes() {
        assert_ne!(pose(Pose::Stand), pose(Pose::Blink));
    }
}
