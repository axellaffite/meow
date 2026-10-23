use super::*;
use crate::sprites::{Bubble, CAT_H, SCRUFF, SWAT_PAW};

const SCALE: f64 = 4.0;
const PX: usize = SCALE as usize;

fn look(pose: Pose) -> Look {
    Look {
        pose,
        bubble: Bubble::None,
        facing: Facing::Right,
    }
}

/// Width and alpha of each pixel of `look`, top row first.
fn coverage(look: &Look, coat: Coat) -> (usize, Vec<u8>) {
    let bitmap = render_look(look, coat, SCALE).unwrap();
    let alpha = bitmap.rgba().chunks(4).map(|px| px[3]).collect();
    (bitmap.width, alpha)
}

/// Whether the sprite pixel at (`x`, `y`) is painted.
fn painted(look: &Look, coat: Coat, (x, y): (usize, usize)) -> bool {
    let (w, alpha) = coverage(look, coat);
    (y * PX..(y + 1) * PX).any(|py| (x * PX..(x + 1) * PX).any(|px| alpha[py * w + px] > 128))
}

fn rows_painted(look: &Look, coat: Coat) -> Vec<bool> {
    let (w, alpha) = coverage(look, coat);
    alpha
        .chunks(w)
        .map(|row| row.iter().any(|&a| a > 128))
        .collect()
}

fn columns_painted(look: &Look, coat: Coat) -> Vec<bool> {
    let (w, alpha) = coverage(look, coat);
    (0..w)
        .map(|x| alpha.chunks(w).any(|row| row[x] > 128))
        .collect()
}

#[test]
fn standing_animals_touch_the_ground_and_leave_the_bubble_space_empty() {
    for coat in Coat::ALL {
        for pose in [
            Pose::Stand,
            Pose::Walk(0),
            Pose::Walk(1),
            Pose::Sleep,
            Pose::Eat(0),
        ] {
            let rows = rows_painted(&look(pose), coat);
            assert!(rows[rows.len() - 1], "{coat:?} {pose:?} touches the ground");
            let bubble = BUBBLE_H * PX;
            assert!(!rows[..bubble - 1].iter().any(|&r| r), "{coat:?} {pose:?}");
        }
    }
}

#[test]
fn left_facing_is_the_mirror_of_right() {
    let right = look(Pose::Walk(0));
    let left = Look {
        facing: Facing::Left,
        ..right
    };
    let (w, a) = coverage(&right, Coat::Ginger);
    let (_, b) = coverage(&left, Coat::Ginger);
    let mismatched = (0..a.len() / w)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .filter(|&(x, y)| a[y * w + x].abs_diff(b[y * w + w - 1 - x]) > 40)
        .count();
    assert!(mismatched < a.len() / 200, "{mismatched} pixels differ");
}

#[test]
fn clinging_animals_reach_the_wall() {
    for coat in Coat::ALL {
        let columns = columns_painted(&look(Pose::Cling(1)), coat);
        assert!(
            columns[columns.len() - 1],
            "{coat:?} paws on the right edge"
        );
        let back = (CANVAS_W - CAT_H) * PX;
        assert!(columns[back + PX], "{coat:?} the whole body is drawn");
    }
}

#[test]
fn upside_down_animals_hang_from_the_top_edge() {
    for coat in Coat::ALL {
        let rows = rows_painted(&look(Pose::Ceiling(1)), coat);
        assert!(rows[0], "{coat:?}");
        let below = CAT_H * PX + 2;
        assert!(!rows[below..].iter().any(|&r| r), "{coat:?}");
    }
}

#[test]
fn hanging_animals_are_held_at_the_scruff() {
    for coat in Coat::ALL {
        for swing in [-3, 0, 3] {
            let held = look(Pose::Dangle {
                swing,
                kick: Kick::None,
            });
            assert!(painted(&held, coat, SCRUFF), "{coat:?} {swing}");
            let rows = rows_painted(&held, coat);
            assert!(!rows[..SCRUFF.1 * PX].iter().any(|&r| r), "{coat:?}");
        }
    }
}

#[test]
fn slapped_down_paw_covers_the_swat_point() {
    for coat in Coat::ALL {
        assert!(painted(&look(Pose::Swat(1)), coat, SWAT_PAW), "{coat:?}");
    }
}

#[test]
fn bubble_sits_up_top_and_props_on_the_floor() {
    let talking = Look {
        bubble: Bubble::Bang,
        ..look(Pose::Stand)
    };
    assert!(rows_painted(&talking, Coat::Ginger)[2 * PX], "the bubble");
    for prop in [
        Prop::Bowl(3),
        Prop::Mouse(0),
        Prop::Hairball(3),
        Prop::Hairball(1),
        Prop::Poop(3),
        Prop::Poop(1),
    ] {
        let bitmap = render_prop(prop, Facing::Right, SCALE).unwrap();
        let rgba = bitmap.rgba();
        let last_row = &rgba[(bitmap.height - 1) * bitmap.width * 4..];
        assert!(last_row.chunks(4).any(|px| px[3] > 0), "{prop:?}");
    }
}

#[test]
fn floating_zs_stay_inside_the_canvas() {
    for frame in 0..crate::sprites::Z_CYCLE as u8 {
        let asleep = Look {
            bubble: Bubble::Zzz(frame),
            ..look(Pose::Sleep)
        };
        let rows = rows_painted(&asleep, Coat::Ginger);
        assert!(!rows[..PX / 4].iter().any(|&r| r), "frame {frame}");
    }
}

#[test]
fn eating_head_reaches_over_the_bowl() {
    // The bowl starts two sprite pixels before the canvas's right edge,
    // its rim on row `CANVAS_H - PROP_H + 2`.
    let rim = CANVAS_H - crate::sprites::PROP_H + 2;
    for coat in Coat::ALL {
        for bite in [0, 1] {
            let eating = look(Pose::Eat(bite));
            let over_bowl = (CANVAS_W - 2..CANVAS_W)
                .any(|x| (rim..rim + 2).any(|y| painted(&eating, coat, (x, y))));
            assert!(over_bowl, "{coat:?} bite {bite}");
        }
    }
}

#[test]
fn blinking_shuts_the_eyes() {
    for coat in Coat::ALL {
        let open = render_look(&look(Pose::Stand), coat, SCALE).unwrap().rgba();
        let shut = render_look(&look(Pose::Blink), coat, SCALE).unwrap().rgba();
        let changed = open
            .chunks(4)
            .zip(shut.chunks(4))
            .filter(|(a, b)| a != b)
            .count();
        assert!(changed > PX * PX, "{coat:?}: {changed} pixels");
    }
}
