//! The props, flat like the cat: the kibble bowl, the mouse, the hairball,
//! its glints once wiped, and Dédé's poop. A few shapes each, no outline and
//! no gradient, drawn facing right on the prop's canvas, sitting on its
//! bottom edge.

use crate::sprites::{Coat, PROP_H, Prop};

use super::paint::{Color, P, Pen, Shape};

const FLOOR: f64 = PROP_H as f64;

pub(super) fn draw(pen: &Pen, prop: Prop) {
    match prop {
        Prop::Bowl(level) => bowl(pen, level.min(3)),
        Prop::Mouse(phase) => mouse(pen, phase),
        Prop::Hairball(dirt) => hairball(pen, dirt.clamp(1, 3)),
        Prop::Sparkle(frame) => sparkle(pen, frame),
        Prop::Poop(left) => poop(pen, left.clamp(1, 3)),
    }
}

fn ink() -> Color {
    Color::rgb(Coat::Ginger.color('k').unwrap_or([0, 0, 0]))
}

/// Kibble heaped above the rim, fullest last: a centre and a tint each.
const KIBBLES: [(P, bool); 9] = [
    ((4.0, 2.3), false),
    ((7.0, 2.3), true),
    ((5.5, 2.1), true),
    ((3.0, 2.4), true),
    ((8.0, 2.4), false),
    ((4.8, 1.5), false),
    ((6.3, 1.4), false),
    ((5.6, 0.8), true),
    ((3.9, 1.7), true),
];

fn bowl(pen: &Pen, level: u8) {
    let blue = Color::rgb([86, 140, 206]);
    let count = [0, 2, 5, 9][usize::from(level)];
    for &((x, y), dark) in &KIBBLES[..count] {
        let color = Color::rgb(if dark { [150, 92, 46] } else { [196, 132, 70] });
        pen.fill(&Shape::ellipse((x, y + 0.3), 0.8, 0.6), color);
    }
    let body = Shape::at((1.0, 2.6))
        .line((10.0, 2.6))
        .curve((10.0, 4.4), (8.6, FLOOR), (7.2, FLOOR))
        .line((3.8, FLOOR))
        .curve((2.4, FLOOR), (1.0, 4.4), (1.0, 2.6))
        .close();
    pen.fill(&body, blue);
    // A band of the lighter blue under the rim.
    pen.clipped(&body, |pen| {
        let band = Shape::at((0.0, 3.6)).line((11.0, 3.6));
        pen.stroke(&band, blue.lighter(0.25), 0.7);
    });
    pen.fill(&Shape::ellipse((5.5, 2.6), 4.6, 0.6), blue.lighter(0.4));
}

fn mouse(pen: &Pen, phase: u8) {
    let grey = Color::rgb([166, 160, 172]);
    let pink = Color::rgb(Coat::Ginger.color('p').unwrap_or([244, 166, 183]));
    let tail = Shape::at((2.2, 4.6)).curve((0.8, 4.9), (0.2, 3.6), (0.5, 2.4));
    pen.stroke(&tail, pink.darker(0.1), 0.45);
    let feet: [f64; 2] = if phase.is_multiple_of(2) {
        [2.9, 7.6]
    } else {
        [3.7, 6.8]
    };
    for x in feet {
        pen.fill(
            &Shape::ellipse((x, FLOOR - 0.4), 0.6, 0.4),
            pink.darker(0.1),
        );
    }
    let body = Shape::at((10.7, 4.4))
        .curve((10.0, 3.0), (8.6, 2.0), (6.6, 2.0))
        .curve((3.8, 2.0), (2.0, 3.4), (2.0, 4.6))
        .curve((2.0, 5.4), (3.0, 5.6), (5.0, 5.6))
        .line((9.0, 5.6))
        .curve((10.0, 5.6), (10.8, 5.0), (10.7, 4.4))
        .close();
    pen.fill(&body, grey);
    pen.fill(&Shape::circle((7.6, 2.0), 1.0), grey.darker(0.12));
    pen.fill(&Shape::circle((7.6, 2.05), 0.55), pink);
    pen.fill(&Shape::circle((9.0, 3.5), 0.32), ink());
    pen.fill(&Shape::circle((10.65, 4.4), 0.34), pink.darker(0.15));
}

fn hairball(pen: &Pen, dirt: u8) {
    let brown = Color::rgb([110, 86, 70]);
    let light = Color::rgb([150, 124, 102]);
    let wet = Color::rgb([200, 196, 160]);
    let (rx, ry, puddle) = match dirt {
        3 => (3.4, 2.1, 5.2),
        2 => (2.4, 1.5, 4.4),
        _ => (0.0, 0.0, 3.6),
    };
    pen.fill(&Shape::ellipse((5.5, FLOOR - 0.55), puddle, 0.55), wet);
    if rx == 0.0 {
        for (x, y) in [(4.0, FLOOR - 1.2), (6.6, FLOOR - 1.0)] {
            let bit = Shape::at((x, y)).quad((x + 0.5, y - 0.6), (x + 1.0, y - 0.2));
            pen.stroke(&bit, brown, 0.4);
        }
        return;
    }
    let center = (5.5, FLOOR - 0.6 - ry);
    pen.fill(&Shape::ellipse(center, rx, ry), brown);
    // Loose hairs, a lighter brown.
    for (i, t) in [-0.6, -0.15, 0.3, 0.7].into_iter().enumerate() {
        let x = center.0 + t * rx;
        let y = center.1 - ry * 0.4 + (i % 2) as f64 * 0.5;
        let hair = Shape::at((x - 0.6, y + 0.3)).quad((x, y - 0.4), (x + 0.6, y + 0.1));
        pen.stroke(&hair, light, 0.22);
    }
    let tuft = Shape::at((center.0 - 0.3, center.1 - ry + 0.1)).quad(
        (center.0 - 0.2, center.1 - ry - 0.8),
        (center.0 + 0.5, center.1 - ry - 0.9),
    );
    pen.stroke(&tuft, brown, 0.28);
}

/// Glints where the hairball was: four-pointed stars that trade places.
fn sparkle(pen: &Pen, frame: u8) {
    let gold = Color::rgb([255, 214, 92]);
    let pale = Color::rgb([255, 238, 170]);
    let stars: [(P, f64, bool); 3] = if frame.is_multiple_of(2) {
        [
            ((3.5, 1.5), 1.4, true),
            ((8.5, 2.5), 1.0, false),
            ((1.5, 4.5), 1.0, false),
        ]
    } else {
        [
            ((8.5, 1.5), 1.4, true),
            ((1.5, 2.5), 1.0, false),
            ((4.5, 4.5), 1.2, true),
        ]
    };
    for ((x, y), r, golden) in stars {
        let w = r * 0.3;
        let star = Shape::at((x, y - r))
            .quad((x + w, y - w), (x + r, y))
            .quad((x + w, y + w), (x, y + r))
            .quad((x - w, y + w), (x - r, y))
            .quad((x - w, y - w), (x, y - r))
            .close();
        pen.fill(&star, if golden { gold } else { pale });
    }
}

/// A soft swirl, in up to three tiers, most of it left first.
fn poop(pen: &Pen, left: u8) {
    let brown = Color::rgb([128, 84, 48]);
    let tiers: &[(f64, f64, f64)] = match left {
        3 => &[(4.6, 1.15, 4.7), (3.4, 0.95, 3.0), (2.0, 0.8, 1.6)],
        2 => &[(3.6, 1.0, 4.9), (2.3, 0.8, 3.3)],
        _ => &[(3.0, 0.6, 5.3)],
    };
    // Each tier a shade lighter than the one under it, so they read apart.
    for (i, &(rx, ry, cy)) in tiers.iter().enumerate() {
        let tier = Shape::ellipse((5.5, cy), rx * 0.95, ry);
        pen.fill(&tier, brown.lighter(0.1 * i as f64));
    }
    if left == 3 {
        let tip = Shape::at((5.2, 0.9)).quad((5.6, 0.1), (6.2, 0.4));
        pen.stroke(&tip, brown.lighter(0.2), 0.6);
    }
}
