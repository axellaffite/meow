//! What floats above the cat, flat like it: a rounded white speech bubble
//! with dots or an exclamation mark in the system font, rising hearts and
//! sleepy z's, where the pixel ones are. No outline and no gradient: the
//! bubble stands out by a flat shadow just under it.

use std::ptr::NonNull;

use objc2_core_foundation::{CGAffineTransform, CGRect};
use objc2_core_graphics::CGPath;
use objc2_core_text::{CTFont, CTFontUIFontType};

use crate::sprites::{
    BUBBLE_H, BUBBLE_X, Bubble, CANVAS_W, Coat, Facing, HEART_CYCLE, HEART_LANES, HEART_STAGGER,
    SLEEP_TOP, Z_CYCLE, Z_X,
};

use super::paint::{Color, P, Pen, Shape, mirror};

/// The bubble's box, as the pixel one: nine pixels wide from `BUBBLE_X`.
const LEFT: f64 = BUBBLE_X as f64 + 0.3;
const RIGHT: f64 = BUBBLE_X as f64 + 8.7;
const TOP: f64 = 0.35;
const BOTTOM: f64 = 5.6;
const CENTER: P = ((LEFT + RIGHT) / 2.0, (TOP + BOTTOM) / 2.0);
/// Height of the "!" in canvas units.
const BANG_SIZE: f64 = 4.4;
/// The highest a z rises: its top stays this far inside the canvas.
const Z_MARGIN: f64 = 0.5;
/// How far below the bubble its flat shadow shows.
const SHADOW: f64 = 0.4;

pub(super) fn draw(pen: &Pen, bubble: Bubble, facing: Facing) {
    let side = |pen: &Pen, draw: &dyn Fn(&Pen)| match facing {
        Facing::Right => draw(pen),
        Facing::Left => pen.transformed(mirror(CANVAS_W as f64), |pen| draw(pen)),
    };
    match bubble {
        Bubble::None => {}
        Bubble::Dots(n) => side(pen, &|pen| {
            speech(pen);
            for i in 0..3u8 {
                let x = CENTER.0 + 2.0 * (f64::from(i) - 1.0);
                let color = if i < n { ink() } else { ink().alpha(0.18) };
                pen.fill(&Shape::circle((x, CENTER.1), 0.6), color);
            }
        }),
        Bubble::Bang => {
            side(pen, &|pen| speech(pen));
            // The glyph itself is never mirrored.
            let x = match facing {
                Facing::Right => CENTER.0,
                Facing::Left => CANVAS_W as f64 - CENTER.0,
            };
            bang(pen, (x, CENTER.1));
        }
        Bubble::Hearts(frame) => side(pen, &|pen| hearts(pen, frame)),
        Bubble::Zzz(frame) => side(pen, &|pen| zs(pen, frame)),
    }
}

fn ink() -> Color {
    Color::rgb(Coat::Ginger.color('k').unwrap_or([0, 0, 0]))
}

/// The rounded box with its tail pointing down at the cat's head.
fn speech(pen: &Pen) {
    let r = 2.0;
    let k = r * 0.552;
    let tail_x = crate::sprites::BUBBLE_TAIL_X as f64 + 0.5;
    let shape = Shape::at((LEFT + r, TOP))
        .line((RIGHT - r, TOP))
        .curve((RIGHT - r + k, TOP), (RIGHT, TOP + r - k), (RIGHT, TOP + r))
        .line((RIGHT, BOTTOM - r))
        .curve(
            (RIGHT, BOTTOM - r + k),
            (RIGHT - r + k, BOTTOM),
            (RIGHT - r, BOTTOM),
        )
        .line((tail_x + 0.9, BOTTOM))
        .quad((tail_x - 0.1, BOTTOM + 0.5), (tail_x - 0.6, BOTTOM + 1.7))
        .quad((tail_x - 0.5, BOTTOM + 0.6), (tail_x - 1.0, BOTTOM))
        .line((LEFT + r, BOTTOM))
        .curve(
            (LEFT + r - k, BOTTOM),
            (LEFT, BOTTOM - r + k),
            (LEFT, BOTTOM - r),
        )
        .line((LEFT, TOP + r))
        .curve((LEFT, TOP + r - k), (LEFT + r - k, TOP), (LEFT + r, TOP))
        .close();
    pen.transformed(super::paint::translate(0.0, SHADOW), |pen| {
        pen.fill(&shape, ink().alpha(0.22))
    });
    pen.fill(&shape, Color::WHITE);
}

/// "!" in the bold system font, centred on `center`; a drawn one if the
/// font has no glyph.
fn bang(pen: &Pen, center: P) {
    match glyph_path('!') {
        Some(path) => {
            let bounds = CGPath::bounding_box(Some(&path));
            let scale = BANG_SIZE / bounds.size.height.max(1.0);
            // Font units grow up; the canvas grows down.
            let transform = CGAffineTransform {
                a: scale,
                b: 0.0,
                c: 0.0,
                d: -scale,
                tx: center.0 - scale * (bounds.origin.x + bounds.size.width / 2.0),
                ty: center.1 + scale * (bounds.origin.y + bounds.size.height / 2.0),
            };
            pen.transformed(transform, |pen| pen.fill_path(&path, ink()));
        }
        None => {
            let (x, y) = center;
            let bar = Shape::at((x, y - 1.8)).line((x, y + 0.6));
            pen.stroke(&bar, ink(), 0.9);
            pen.fill(&Shape::circle((x, y + 1.8), 0.5), ink());
        }
    }
}

/// The outline of `c` in the bold system font, in font units.
fn glyph_path(c: char) -> Option<objc2_core_foundation::CFRetained<CGPath>> {
    let mut units = [0u16; 2];
    let units = c.encode_utf16(&mut units);
    let mut glyph = 0u16;
    // SAFETY: a system font at 64 points; the character and glyph buffers
    // each hold the one value `count` says, and a null matrix means none.
    unsafe {
        let font =
            CTFont::new_ui_font_for_language(CTFontUIFontType::EmphasizedSystem, 64.0, None)?;
        let found = font.glyphs_for_characters(
            NonNull::new(units.as_mut_ptr())?,
            NonNull::from(&mut glyph),
            1,
        );
        if !found {
            return None;
        }
        let path = font.path_for_glyph(glyph, std::ptr::null())?;
        let bounds: CGRect = CGPath::bounding_box(Some(&path));
        (bounds.size.height > 0.0).then_some(path)
    }
}

fn heart(center: P, size: f64) -> Shape {
    let (x, y) = center;
    let s = size / 2.0;
    Shape::at((x, y + s * 0.95))
        .curve(
            (x - s * 0.5, y + s * 0.6),
            (x - s * 1.1, y + s * 0.1),
            (x - s * 1.0, y - s * 0.45),
        )
        .curve(
            (x - s * 0.9, y - s * 1.0),
            (x - s * 0.2, y - s * 1.05),
            (x, y - s * 0.5),
        )
        .curve(
            (x + s * 0.2, y - s * 1.05),
            (x + s * 0.9, y - s * 1.0),
            (x + s * 1.0, y - s * 0.45),
        )
        .curve(
            (x + s * 1.1, y + s * 0.1),
            (x + s * 0.5, y + s * 0.6),
            (x, y + s * 0.95),
        )
        .close()
}

/// Hearts rise through the space above the cat in three staggered lanes,
/// appearing from below and leaving through the top, as the pixel ones do.
fn hearts(pen: &Pen, frame: u8) {
    let red = Color::rgb(Coat::Ginger.color('r').unwrap_or([224, 67, 90]));
    let space = Shape::at((0.0, 0.0))
        .line((CANVAS_W as f64, 0.0))
        .line((CANVAS_W as f64, BUBBLE_H as f64))
        .line((0.0, BUBBLE_H as f64))
        .close();
    pen.clipped(&space, |pen| {
        for (lane, &x) in HEART_LANES.iter().enumerate() {
            let progress = (usize::from(frame) + lane * HEART_STAGGER) % HEART_CYCLE;
            let top = BUBBLE_H as f64 - progress as f64;
            let wobble = (progress / 4 % 2) as f64;
            let center = (x as f64 + 2.5 + wobble, top + 2.0);
            let shape = heart(center, 4.4);
            let fade = (top + 4.0) / BUBBLE_H as f64;
            pen.fill(&shape, red.alpha(fade.clamp(0.35, 1.0)));
        }
    });
}

/// Two z's take turns floating up from just above the sleeping cat's head,
/// drifting back over its body, shrinking and fading before the top edge
/// so neither is ever cut off.
fn zs(pen: &Pen, frame: u8) {
    let space = Shape::at((0.0, 0.0))
        .line((CANVAS_W as f64, 0.0))
        .line((CANVAS_W as f64, SLEEP_TOP as f64))
        .line((0.0, SLEEP_TOP as f64))
        .close();
    pen.clipped(&space, |pen| {
        for offset in [0, Z_CYCLE / 2] {
            let progress = (usize::from(frame) + offset) % Z_CYCLE;
            // The cycle's last frame is the gap between two z's.
            if progress == Z_CYCLE - 1 {
                continue;
            }
            // From just above the head to the top edge over the cycle.
            let rise = progress as f64 / (Z_CYCLE - 2) as f64;
            let start = (SLEEP_TOP - 5) as f64 + 0.7;
            // Smaller as they rise.
            let size = 1.0 - rise * 0.35;
            let (w, h) = (2.6 * size, 3.6 * size);
            let y = start - rise * (start - Z_MARGIN);
            let x = Z_X as f64 + 0.2 - (progress / 2) as f64;
            let z = Shape::at((x, y))
                .line((x + w, y))
                .line((x, y + h))
                .line((x + w, y + h));
            let alpha = (1.0 - rise * 0.65).clamp(0.35, 1.0);
            pen.stroke(&z, ink().alpha(alpha), 0.6);
        }
    });
}
