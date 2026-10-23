//! The cat itself, as a flat mascot: a few rounded shapes in the coat's
//! colors, no outline and no gradient, and a face of two little eyes and a
//! nose. One parameterised standing cat that every upright pose bends, the
//! loaf asleep and the cat held up by the scruff, seen from the front.

use crate::sprites::{CANVAS_H, Coat};

use super::paint::{Color, P, Pen, Shape, rotate_about, scale_about, translate};
use super::stance::Stance;

/// The ground: the bottom edge of the canvas.
const GROUND: f64 = CANVAS_H as f64;
/// Where each leg leaves the body, far hind, near hind, far front, near
/// front: over the pixel cat's legs.
const HIPS: [f64; 4] = [5.0, 7.6, 14.6, 17.2];
const FAR: [bool; 4] = [true, false, true, false];
/// Top of the legs, inside the body.
const HIP_Y: f64 = 17.6;
const LEG_WIDTH: f64 = 2.2;
/// A leg's round end rests on the ground.
const PAW_Y: f64 = GROUND - LEG_WIDTH / 2.0;
/// How much of a leg, from the paw up, a sock of another color covers.
const SOCK: f64 = 0.45;
/// Centre of the standing cat's head, and the neck it turns about.
const HEAD: P = (18.5, 12.6);
const NECK: P = (16.8, 14.6);
/// How much of a stance's tilt the head follows.
const HEAD_NOD: f64 = 0.4;
/// The body: a rounded slab from the rump to the chest.
const BODY_LEFT: f64 = 3.0;
const BODY_RIGHT: f64 = 19.8;
const BODY_TOP: f64 = 12.8;
const BODY_BOTTOM: f64 = 19.4;
const BODY_ROUND: f64 = 3.2;

/// The coat's colors, ready to paint flat.
pub(super) struct Inks {
    pub fur: Color,
    /// The fur a shade deeper: far legs, the inside of the ears, stripes
    /// when the coat has none of its own.
    pub deep: Color,
    /// Tabby stripes, if the coat has them.
    pub stripe: Option<Color>,
    /// The forehead, if not the fur.
    pub blaze: Option<Color>,
    pub muzzle: Color,
    /// Socks, if the legs are not the fur.
    pub socks: Option<Color>,
    pub tail: Color,
    pub eyes: [Color; 2],
    pub ink: Color,
}

impl Inks {
    pub fn of(coat: Coat) -> Self {
        let palette = coat.palette();
        let part = |c: char| Color::rgb(coat.color(c).unwrap_or([0, 0, 0]));
        let fur = Color::rgb(palette.fur);
        let unless_fur = |rgb| (rgb != palette.fur).then(|| Color::rgb(rgb));
        let stripe = unless_fur(palette.stripe);
        // A shade deeper than the fur, toward the stripes when there are.
        let deep = match stripe {
            Some(stripe) => fur.mix(stripe, 0.75),
            None => fur.darker(if fur.luma() > 0.6 { 0.12 } else { 0.18 }),
        };
        let white = part('w');
        // A light coat shows its stripes and points in the darker tail; a
        // tail of the fur's own color is drawn a shade deeper so it reads.
        let tail = unless_fur(palette.tail).unwrap_or(deep);
        Self {
            fur,
            deep,
            stripe,
            blaze: unless_fur(palette.blaze),
            muzzle: unless_fur(palette.muzzle).unwrap_or(white),
            socks: unless_fur(palette.legs),
            tail,
            eyes: [Color::rgb(palette.near_eye), Color::rgb(palette.far_eye)],
            ink: part('k'),
        }
    }

    /// The far legs, a shade deeper than the near ones.
    fn far(&self, color: Color) -> Color {
        color.darker(0.13)
    }
}

/// A flat triangle with its corners rounded by `round`.
pub(super) fn soft_triangle(pen: &Pen, points: [P; 3], color: Color, round: f64) {
    let shape = Shape::at(points[0]).line(points[1]).line(points[2]).close();
    pen.fill(&shape, color);
    pen.stroke(&shape, color, round);
}

/// A soft arc from `a` to `b`, bowed `bow` toward y down (negative: up).
fn arc(a: P, b: P, bow: f64) -> Shape {
    let mid = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0 + 2.0 * bow);
    Shape::at(a).quad(mid, b)
}

pub(super) fn standing(pen: &Pen, coat: Coat, stance: &Stance) {
    let inks = Inks::of(coat);
    tail(pen, &inks, stance);
    for i in [0, 2] {
        leg(pen, &inks, stance, i);
    }
    body(pen, &inks, stance);
    let raised = |i: usize| stance.legs[i].paw.is_some();
    for i in [1, 3] {
        if !raised(i) {
            leg(pen, &inks, stance, i);
        }
    }
    let (dx, dy) = stance.head;
    pen.transformed(translate(dx, dy), |pen| {
        // The face looks at the viewer, so it only nods a little of the tilt.
        pen.transformed(rotate_about(NECK, stance.tilt * HEAD_NOD), |pen| {
            pen.transformed(translate(HEAD.0, HEAD.1), |pen| {
                head(pen, &inks, stance.closed, stance.mouth_open, 0.0)
            })
        })
    });
    // A raised paw comes up in front of the chin.
    for i in [1, 3] {
        if raised(i) {
            leg(pen, &inks, stance, i);
        }
    }
}

fn tail(pen: &Pen, inks: &Inks, stance: &Stance) {
    let r = stance.rump;
    let shape = if stance.tail_up {
        Shape::at((4.6, 15.4 + r)).curve((2.0, 14.2), (1.6, 10.8), (2.8, 9.2))
    } else {
        Shape::at((4.6, 15.6 + r)).curve((1.6, 15.6), (1.2, 12.0), (2.4, 10.0))
    };
    pen.stroke(&shape, inks.tail, 1.9);
}

fn leg(pen: &Pen, inks: &Inks, stance: &Stance, i: usize) {
    let leg = stance.legs[i];
    let top_y = HIP_Y + if i < 2 { stance.rump * 0.6 } else { 0.0 };
    let (top, paw) = match leg.paw {
        // A raised paw swings from the shoulder.
        Some(paw) => ((HIPS[i], top_y - 1.0), paw),
        None => ((HIPS[i], top_y), (HIPS[i] + leg.dx, PAW_Y - leg.lift)),
    };
    let shade = |color: Color| if FAR[i] { inks.far(color) } else { color };
    pen.stroke(&Shape::at(top).line(paw), shade(inks.fur), LEG_WIDTH);
    if let Some(socks) = inks.socks {
        let ankle = (
            paw.0 + (top.0 - paw.0) * SOCK * 0.6,
            paw.1 + (top.1 - paw.1) * SOCK * 0.6,
        );
        pen.stroke(&Shape::at(ankle).line(paw), shade(socks), LEG_WIDTH);
    }
}

/// The body, its back raised by `hunch` and its rump lowered by `rump`.
fn body_shape(hunch: f64, rump: f64) -> Shape {
    let (l, r, t, b, k) = (BODY_LEFT, BODY_RIGHT, BODY_TOP, BODY_BOTTOM, BODY_ROUND);
    // A quarter circle is a cubic with handles this far along its tangents.
    let h = k * 0.552;
    let (back_t, back_b) = (t + rump * 0.8, b + rump * 0.3);
    Shape::at((l + k, back_t))
        .curve(
            (l + k + 4.0, t - hunch),
            (r - k - 4.0, t - hunch),
            (r - k, t),
        )
        .curve((r - k + h, t), (r, t + k - h), (r, t + k))
        .line((r, b - k))
        .curve((r, b - k + h), (r - k + h, b), (r - k, b))
        .line((l + k, back_b))
        .curve((l + k - h, back_b), (l, back_b - k + h), (l, back_b - k))
        .line((l, back_t + k))
        .curve((l, back_t + k - h), (l + k - h, back_t), (l + k, back_t))
        .close()
}

fn body(pen: &Pen, inks: &Inks, stance: &Stance) {
    let shape = body_shape(stance.hunch, stance.rump);
    pen.fill(&shape, inks.fur);
    pen.clipped(&shape, |pen| {
        let stripe = inks.stripe.unwrap_or(inks.deep);
        if inks.stripe.is_some() {
            for x in [7.4, 10.0, 12.6] {
                let top = BODY_TOP - stance.hunch * 0.8;
                let bar = Shape::at((x, top - 1.0)).line((x, top + 1.5));
                pen.stroke(&bar, stripe, 1.15);
            }
        }
        // The chest.
        pen.fill(&Shape::ellipse((19.2, 15.6), 1.6, 1.7), inks.muzzle);
    });
}

/// The head, a rounded square centred on the origin facing the viewer,
/// its ears turned `ears_out` radians outward.
pub(super) fn head(pen: &Pen, inks: &Inks, closed: bool, mouth_open: bool, ears_out: f64) {
    // A darker blaze, as on a point coat, colors the ears; a light one
    // runs down the face (see `markings`).
    let dark_blaze = inks.blaze.filter(|blaze| blaze.luma() < inks.fur.luma());
    let ear = dark_blaze.unwrap_or(inks.fur);
    let inner_ear = dark_blaze.map_or(inks.deep, |blaze| blaze.lighter(0.3));
    for side in [-1.0, 1.0] {
        let s = |(x, y): P| (x * side, y);
        pen.transformed(rotate_about(s((-2.4, -2.6)), -ears_out * side), |pen| {
            soft_triangle(
                pen,
                [s((-3.75, -1.0)), s((-3.2, -4.4)), s((-0.8, -2.9))],
                ear,
                0.9,
            );
            soft_triangle(
                pen,
                [s((-3.1, -1.9)), s((-2.85, -3.5)), s((-1.7, -2.75))],
                inner_ear,
                0.5,
            );
        });
    }
    let shape = Shape::rounded(-4.0, -3.4, 4.0, 3.2, 3.0);
    pen.fill(&shape, inks.fur);
    pen.clipped(&shape, |pen| markings(pen, inks));
    for (side, iris) in [(-1.0, inks.eyes[1]), (1.0, inks.eyes[0])] {
        eye(pen, inks, (1.6 * side, 0.0), iris, closed);
    }
    if mouth_open {
        pen.fill(&Shape::ellipse((0.0, 2.25), 0.5, 0.45), inks.ink);
    }
    pen.fill(&Shape::ellipse((0.0, 1.15), 0.42, 0.32), inks.ink);
}

/// The forehead, the tabby bars and the muzzle, inside the head.
fn markings(pen: &Pen, inks: &Inks) {
    if let Some(blaze) = inks.blaze.filter(|blaze| blaze.luma() > inks.fur.luma()) {
        // A light blaze runs down between the eyes, widening to the muzzle.
        let wedge = Shape::at((-0.45, -3.6))
            .line((0.45, -3.6))
            .quad((0.5, -0.6), (1.3, 1.0))
            .line((-1.3, 1.0))
            .quad((-0.5, -0.6), (-0.45, -3.6))
            .close();
        pen.fill(&wedge, blaze);
    }
    if let Some(stripe) = inks.stripe {
        for x in [-0.9, 0.0, 0.9] {
            let bar = Shape::at((x, -3.6)).line((x, -2.6));
            pen.stroke(&bar, stripe, 0.5);
        }
    }
    pen.fill(&Shape::ellipse((0.0, 2.2), 2.3, 1.5), inks.muzzle);
}

/// A short upright capsule; in the coat's eye color with a dark pupil when
/// that is light enough to tell, a soft downward curve when shut.
fn eye(pen: &Pen, inks: &Inks, (x, y): P, iris: Color, closed: bool) {
    if closed {
        let lid = arc((x - 0.55, y), (x + 0.55, y), 0.28);
        pen.stroke(&lid, inks.ink, 0.36);
        return;
    }
    let capsule = |half: f64| Shape::at((x, y - half)).line((x, y + half));
    if iris.luma() < 0.3 {
        pen.stroke(&capsule(0.3), inks.ink, 0.78);
    } else {
        pen.stroke(&capsule(0.32), iris, 1.0);
        pen.stroke(&capsule(0.3), inks.ink, 0.45);
    }
}

/// Asleep: a flat loaf on the floor, the head resting at its front, eyes
/// shut, the tail along the floor.
pub(super) fn sleeping(pen: &Pen, coat: Coat) {
    let inks = Inks::of(coat);
    let body = Shape::rounded(2.4, 14.4, 20.6, GROUND + 2.0, 4.0);
    // Its rounded bottom runs off the canvas, so it sits flat on the floor.
    pen.clipped(&Shape::rounded(0.0, 0.0, 24.0, GROUND, 0.0), |pen| {
        pen.fill(&body, inks.fur);
        pen.clipped(&body, |pen| {
            if let Some(stripe) = inks.stripe {
                for x in [6.2, 8.8, 11.4] {
                    let bar = Shape::at((x, 13.4)).line((x, 16.0));
                    pen.stroke(&bar, stripe, 1.15);
                }
            }
        });
    });
    let tail = Shape::at((3.4, GROUND - 0.9)).line((14.8, GROUND - 0.9));
    pen.stroke(&tail, inks.tail, 1.8);
    let paws = inks.socks.unwrap_or(inks.fur);
    for x in [16.6, 19.6] {
        pen.fill(&Shape::ellipse((x, GROUND - 0.7), 1.2, 0.7), paws);
    }
    pen.transformed(translate(18.0, 16.9), |pen| {
        pen.transformed(scale_about((0.0, 0.0), 0.95), |pen| {
            head(pen, &inks, true, false, 0.1)
        })
    });
}

/// Hind legs of the hanging cat.
const HANG_HIPS: [f64; 2] = [10.4, 13.6];

/// Held up by the scruff, seen from the front: a rounded upright body,
/// front paws dangling, one hind leg pulled up mid-kick.
pub(super) fn hanging(pen: &Pen, coat: Coat, kicked: Option<usize>) {
    let inks = Inks::of(coat);
    let tail = Shape::at((14.0, 15.6)).curve((16.2, 16.6), (16.6, 18.8), (16.4, 20.8));
    pen.stroke(&tail, inks.tail, 1.8);
    let socks = inks.socks.unwrap_or(inks.fur);
    for (i, x) in HANG_HIPS.into_iter().enumerate() {
        let side = if i == 0 { -1.0 } else { 1.0 };
        let (shape, paw) = if kicked == Some(i) {
            let paw = (x + 1.4 * side, 18.0);
            (Shape::at((x, 15.0)).quad((x + 1.8 * side, 16.0), paw), paw)
        } else {
            let paw = (x, 19.8);
            (Shape::at((x, 15.0)).line(paw), paw)
        };
        pen.stroke(&shape, inks.far(inks.fur), LEG_WIDTH);
        if inks.socks.is_some() {
            let ankle = (paw.0, paw.1 - 1.2);
            pen.stroke(&Shape::at(ankle).line(paw), inks.far(socks), LEG_WIDTH);
        }
    }
    let body = Shape::rounded(8.6, 8.0, 15.4, 17.2, 3.2);
    pen.fill(&body, inks.fur);
    pen.clipped(&body, |pen| {
        pen.fill(&Shape::ellipse((12.0, 13.4), 2.3, 3.0), inks.muzzle);
    });
    // The front paws hang down the sides.
    for x in [8.9, 15.1] {
        pen.stroke(&Shape::at((x, 10.0)).line((x, 13.4)), inks.fur, 2.0);
        if inks.socks.is_some() {
            pen.fill(&Shape::circle((x, 13.5), 1.0), socks);
        }
    }
    pen.fill(&scruff(3.6), inks.fur);
    pen.transformed(translate(12.0, 6.0), |pen| {
        pen.transformed(scale_about((0.0, 0.0), 0.92), |pen| {
            head(pen, &inks, false, false, 0.4)
        })
    });
}

/// The pinched skin of the neck, from the head at `base` up to the fingers
/// holding it at `SCRUFF`: a rounded lock rather than a point.
pub(super) fn scruff(base: f64) -> Shape {
    Shape::at((11.0, base))
        .curve((11.5, base - 1.0), (11.3, 1.7), (11.65, 1.1))
        .quad((12.05, 0.75), (12.45, 1.1))
        .curve((12.8, 1.7), (12.6, base - 1.0), (13.1, base))
        .close()
}
