//! Dédé the pig, flat like the cat: a round pink body, a deeper pink for
//! the snout, ears, far legs, hooves and curly tail, a pale belly, and mud
//! on his flank; no outline and no gradient.

use crate::sprites::{CANVAS_H, Coat};

use super::cat::{Inks, scruff, soft_triangle};
use super::paint::{Color, P, Pen, Shape, rotate_about, translate};
use super::stance::{BOW_PIVOT, Stance};

const GROUND: f64 = CANVAS_H as f64;
/// Where each leg leaves the body, far hind, near hind, far front, near front.
const HIPS: [f64; 4] = [4.6, 7.2, 14.0, 16.6];
const FAR: [bool; 4] = [true, false, true, false];
const HIP_Y: f64 = 18.0;
const LEG_WIDTH: f64 = 2.2;
const HOOF_H: f64 = 0.9;
/// Centre of his head when standing, and where it turns.
const HEAD: P = (17.4, 13.6);
const NECK: P = (15.9, 15.1);

/// Dédé's own colors.
struct Pink {
    fur: Color,
    deep: Color,
    belly: Color,
    mud: Option<Color>,
    nostril: Color,
    ink: Color,
    cheek: Color,
}

impl Pink {
    fn of(coat: Coat) -> Self {
        let part = |c: char| Color::rgb(coat.color(c).unwrap_or([0, 0, 0]));
        let inks = Inks::of(coat);
        Self {
            fur: inks.fur,
            deep: inks.muzzle,
            belly: part('b'),
            mud: inks.stripe,
            nostril: part('n'),
            ink: inks.ink,
            cheek: part('q'),
        }
    }
}

pub(super) fn standing(pen: &Pen, coat: Coat, stance: &Stance) {
    let pig = Pink::of(coat);
    tail(pen, &pig, (2.0, 13.4 + stance.rump), stance.tail_up);
    for i in [0, 2] {
        leg(pen, &pig, stance, i);
    }
    body(pen, &pig, stance.hunch, stance.rump);
    let raised = |i: usize| stance.legs[i].paw.is_some();
    for i in [1, 3] {
        if !raised(i) {
            leg(pen, &pig, stance, i);
        }
    }
    let (dx, dy) = stance.head;
    pen.transformed(translate(dx, dy), |pen| {
        pen.transformed(rotate_about(NECK, stance.tilt), |pen| {
            pen.transformed(translate(HEAD.0, HEAD.1), |pen| {
                head(pen, &pig, stance.closed, stance.mouth_open)
            })
        })
    });
    for i in [1, 3] {
        if raised(i) {
            leg(pen, &pig, stance, i);
        }
    }
}

fn body_shape(hunch: f64, rump: f64) -> Shape {
    Shape::at((13.0, 11.4))
        .curve((8.5, 10.6 - hunch), (2.4, 11.0 + rump), (1.4, 14.8 + rump))
        .curve((0.7, 18.0 + rump * 0.5), (3.0, 19.9), (7.5, 19.9))
        .line((14.0, 19.9))
        .curve((17.5, 19.9), (19.8, 18.6), (19.6, 15.6))
        .curve((19.4, 12.8), (16.5, 11.8), (13.0, 11.4))
        .close()
}

fn body(pen: &Pen, pig: &Pink, hunch: f64, rump: f64) {
    let shape = body_shape(hunch, rump);
    pen.fill(&shape, pig.fur);
    pen.clipped(&shape, |pen| {
        pen.fill(&Shape::ellipse((11.0, 20.8), 8.4, 2.6), pig.belly);
        if let Some(mud) = pig.mud {
            let splash = Shape::ellipse((6.0, 14.2 + rump * 0.5), 1.7, 1.05)
                .and(Shape::ellipse((8.4, 15.1), 1.0, 0.7))
                .and(Shape::ellipse((4.6, 15.8 + rump * 0.5), 0.6, 0.45));
            pen.fill(&splash, mud);
        }
    });
}

fn leg(pen: &Pen, pig: &Pink, stance: &Stance, i: usize) {
    let leg = stance.legs[i];
    let color = if FAR[i] { pig.fur.darker(0.1) } else { pig.fur };
    let (top, foot) = match leg.paw {
        Some(paw) => ((HIPS[i], HIP_Y - 1.5), (paw.0, paw.1 + 0.5)),
        None => (
            (HIPS[i], HIP_Y + if i < 2 { stance.rump * 0.5 } else { 0.0 }),
            (
                HIPS[i] + leg.dx,
                GROUND - 0.05 - leg.lift - stance.slope * (HIPS[i] + leg.dx - BOW_PIVOT),
            ),
        ),
    };
    let ankle = (foot.0, foot.1 - HOOF_H);
    pen.stroke(&Shape::at(top).line(ankle), color, LEG_WIDTH);
    let hoof = Shape::rounded(
        foot.0 - LEG_WIDTH / 2.0,
        foot.1 - HOOF_H - 0.3,
        foot.0 + LEG_WIDTH / 2.0,
        foot.1,
        0.5,
    );
    let hoof_color = if FAR[i] {
        pig.deep.darker(0.1)
    } else {
        pig.deep
    };
    pen.fill(&hoof, hoof_color);
}

fn tail(pen: &Pen, pig: &Pink, (x, y): P, up: bool) {
    let lift = if up { -1.6 } else { 0.0 };
    let curl = Shape::at((x + 0.4, y))
        .curve(
            (x - 1.0, y - 0.2 + lift),
            (x - 1.4, y - 1.8 + lift),
            (x - 0.4, y - 2.0 + lift),
        )
        .curve(
            (x + 0.5, y - 2.2 + lift),
            (x + 0.5, y - 0.9 + lift),
            (x - 0.4, y - 1.0 + lift),
        );
    pen.stroke(&curl, pig.deep, 0.8);
}

/// His head, side on, centred on the origin, the snout to the right.
fn head(pen: &Pen, pig: &Pink, closed: bool, mouth_open: bool) {
    soft_triangle(
        pen,
        [(-1.8, -2.4), (-0.4, -4.6), (0.9, -2.8)],
        pig.fur.darker(0.1),
        0.8,
    );
    // A deeper rim behind the head sets it apart from the body.
    pen.fill(&Shape::ellipse((-0.5, 0.2), 3.7, 3.4), pig.fur.darker(0.08));
    pen.fill(&Shape::ellipse((0.0, 0.0), 3.7, 3.4), pig.fur);
    let snout = Shape::rounded(2.2, -0.7, 5.3, 2.5, 1.4);
    pen.fill(&snout, pig.deep);
    for y in [0.35, 1.45] {
        pen.fill(&Shape::ellipse((4.7, y), 0.24, 0.34), pig.nostril);
    }
    if mouth_open {
        pen.fill(&Shape::ellipse((3.3, 2.9), 0.55, 0.45), pig.ink);
    }
    pen.fill(&Shape::ellipse((0.5, 1.7), 0.95, 0.55), pig.cheek);
    let eye = (1.1, -0.7);
    if closed {
        let lid =
            Shape::at((eye.0 - 0.55, eye.1)).quad((eye.0, eye.1 + 0.55), (eye.0 + 0.55, eye.1));
        pen.stroke(&lid, pig.ink, 0.36);
    } else {
        let capsule = Shape::at((eye.0, eye.1 - 0.3)).line((eye.0, eye.1 + 0.3));
        pen.stroke(&capsule, pig.ink, 0.78);
    }
    // The near ear flops forward over the brow.
    soft_triangle(pen, [(0.0, -3.0), (2.8, -4.7), (2.4, -1.9)], pig.fur, 0.8);
    soft_triangle(
        pen,
        [(0.9, -3.0), (2.4, -3.95), (2.15, -2.5)],
        pig.deep,
        0.4,
    );
}

/// Asleep, lying down, legs tucked under.
pub(super) fn sleeping(pen: &Pen, coat: Coat) {
    let pig = Pink::of(coat);
    tail(pen, &pig, (2.0, 16.4), false);
    pen.transformed(translate(0.0, 2.05), |pen| body(pen, &pig, 0.0, 0.0));
    pen.transformed(translate(17.4, 16.8), |pen| head(pen, &pig, true, false));
}

/// Held up by the skin of his neck, seen from the front.
pub(super) fn hanging(pen: &Pen, coat: Coat, kicked: Option<usize>) {
    let pig = Pink::of(coat);
    let hoof = |pen: &Pen, (x, y): P, color: Color| {
        let shape = Shape::rounded(x - 1.0, y - 1.1, x + 1.0, y, 0.5);
        pen.fill(&shape, color);
    };
    for (i, x) in [10.0, 14.0].into_iter().enumerate() {
        let side = if i == 0 { -1.0 } else { 1.0 };
        let foot = if kicked == Some(i) {
            (x + 1.2 * side, 18.4)
        } else {
            (x, 20.0)
        };
        let bone = Shape::at((x, 15.0)).line((foot.0, foot.1 - 0.9));
        pen.stroke(&bone, pig.fur.darker(0.1), LEG_WIDTH);
        hoof(pen, foot, pig.deep.darker(0.1));
    }
    let body = Shape::at((9.0, 8.2))
        .curve((7.2, 10.5), (7.0, 14.6), (8.8, 16.3))
        .curve((10.2, 17.4), (13.8, 17.4), (15.2, 16.3))
        .curve((17.0, 14.6), (16.8, 10.5), (15.0, 8.2))
        .close();
    pen.fill(&body, pig.fur);
    pen.clipped(&body, |pen| {
        pen.fill(&Shape::ellipse((12.0, 13.2), 2.6, 3.6), pig.belly);
    });
    tail(pen, &pig, (15.6, 15.8), false);
    for (top, bottom) in [((10.2, 9.4), (10.4, 12.6)), ((13.8, 9.4), (13.6, 12.6))] {
        pen.stroke(&Shape::at(top).line(bottom), pig.fur, 1.9);
        hoof(pen, (bottom.0, bottom.1 + 0.8), pig.deep);
    }
    pen.fill(&scruff(3.6), pig.fur);
    pen.transformed(translate(12.0, 5.9), |pen| front_head(pen, &pig));
}

/// His head from the front: floppy ears, a big snout with two nostrils.
fn front_head(pen: &Pen, pig: &Pink) {
    for side in [-1.0, 1.0] {
        let s = |(x, y): P| (x * side, y);
        soft_triangle(
            pen,
            [s((1.8, -2.8)), s((4.6, -2.6)), s((3.0, -0.8))],
            pig.fur,
            0.8,
        );
        soft_triangle(
            pen,
            [s((2.5, -2.3)), s((3.8, -2.2)), s((3.0, -1.4))],
            pig.deep,
            0.4,
        );
    }
    pen.fill(&Shape::ellipse((0.0, 0.0), 3.8, 3.4), pig.fur);
    for side in [-1.0, 1.0] {
        let (x, y) = (1.6 * side, -1.0);
        pen.stroke(&Shape::at((x, y - 0.3)).line((x, y + 0.3)), pig.ink, 0.78);
        pen.fill(&Shape::ellipse((2.4 * side, 1.0), 0.8, 0.5), pig.cheek);
    }
    pen.fill(&Shape::ellipse((0.0, 1.3), 1.8, 1.3), pig.deep);
    for side in [-1.0, 1.0] {
        pen.fill(&Shape::ellipse((0.6 * side, 1.3), 0.3, 0.45), pig.nostril);
    }
}
