//! The Classic style's cat: a flat vector mascot of the same `Look` the
//! pixel sprites render, rounded shapes in flat colors instead of a grid.
//!
//! It is drawn on the pixel cat's canvas, in sprite pixels with y down, so
//! its feet sit on the bottom edge, its paws reach the wall it clings to and
//! it hangs from `SCRUFF`, as the behaviour code expects. Everything goes
//! through Core Graphics: the cat's window on screen, or an offscreen
//! `Bitmap` for previews and the sidebar icon.

mod bubble;
mod cat;
mod paint;
mod pig;
mod props;
mod stance;

use objc2_core_foundation::CGAffineTransform;
use objc2_core_graphics::CGContext;

use crate::sprites::{BUBBLE_H, CANVAS_H, CANVAS_W, Coat, Facing, Kick, Look, PROP_W, Pose, Prop};

pub use paint::Bitmap;
use paint::{Pen, mirror};
use stance::{BOW_PIVOT, Stance};

/// Draws `look` in `coat` on `ctx`, set up with the canvas's top left corner
/// at the origin, y down and one unit per sprite pixel.
pub fn draw_look(ctx: &CGContext, look: &Look, coat: Coat) {
    let pen = Pen::new(ctx);
    facing(&pen, look.facing, CANVAS_W, |pen| {
        draw_pose(pen, look.pose, coat)
    });
    bubble::draw(&pen, look.bubble, look.facing);
}

/// Draws `prop` on `ctx`, set up like for `draw_look`, on the prop's canvas.
pub fn draw_prop(ctx: &CGContext, prop: Prop, facing_to: Facing) {
    let pen = Pen::new(ctx);
    facing(&pen, facing_to, PROP_W, |pen| props::draw(pen, prop));
}

/// `look` rendered offscreen, `scale` pixels to a sprite pixel.
pub fn render_look(look: &Look, coat: Coat, scale: f64) -> Option<Bitmap> {
    let size = |units: usize| (units as f64 * scale).round() as usize;
    let bitmap = Bitmap::new(size(CANVAS_W), size(CANVAS_H))?;
    bitmap.draw(scale, |ctx| draw_look(ctx, look, coat));
    Some(bitmap)
}

/// `prop` rendered offscreen, `scale` pixels to a sprite pixel.
pub fn render_prop(prop: Prop, facing_to: Facing, scale: f64) -> Option<Bitmap> {
    use crate::sprites::PROP_H;
    let size = |units: usize| (units as f64 * scale).round() as usize;
    let bitmap = Bitmap::new(size(PROP_W), size(PROP_H))?;
    bitmap.draw(scale, |ctx| draw_prop(ctx, prop, facing_to));
    Some(bitmap)
}

/// The art faces right; facing left mirrors it across a canvas `width` wide.
fn facing(pen: &Pen, facing: Facing, width: usize, draw: impl FnOnce(&Pen)) {
    match facing {
        Facing::Right => draw(pen),
        Facing::Left => pen.transformed(mirror(width as f64), draw),
    }
}

/// The standing cat turned a quarter turn, as `sprites` does it: head up,
/// feet on the right edge (the wall), tail hanging off the bottom.
const CLING: CGAffineTransform = CGAffineTransform {
    a: 0.0,
    b: -1.0,
    c: 1.0,
    d: 0.0,
    tx: (CANVAS_W - (CANVAS_H - BUBBLE_H)) as f64 - BUBBLE_H as f64,
    ty: CANVAS_H as f64 + 1.0,
};

/// The standing cat flipped over: feet on the top edge, its back below.
const CEILING: CGAffineTransform = CGAffineTransform {
    a: 1.0,
    b: 0.0,
    c: 0.0,
    d: -1.0,
    tx: 0.0,
    ty: CANVAS_H as f64,
};

fn draw_pose(pen: &Pen, pose: Pose, coat: Coat) {
    let stand = |pen: &Pen, stance: Stance| {
        if coat.is_pig() {
            pig::standing(pen, coat, &stance)
        } else {
            cat::standing(pen, coat, &stance)
        }
    };
    match pose {
        Pose::Walk(phase) => stand(pen, Stance::walking(phase)),
        Pose::Stand => stand(pen, Stance::default()),
        Pose::Blink => stand(pen, Stance::blinking()),
        Pose::Fall => stand(pen, Stance::falling()),
        Pose::Eat(bite) => stand(pen, Stance::eating(bite)),
        Pose::Swat(phase) => stand(pen, Stance::swatting(phase)),
        Pose::Hack(phase) => stand(pen, Stance::hacking(phase)),
        Pose::Squat(phase) => stand(pen, Stance::squatting(phase)),
        Pose::Bow(phase) => {
            let stance = Stance::bowing(phase);
            pen.transformed(tip(stance.slope), |pen| stand(pen, stance))
        }
        Pose::Arch(phase) => stand(pen, Stance::arching(phase)),
        Pose::Scratch(phase) => pen.transformed(CLING, |pen| stand(pen, Stance::scratching(phase))),
        Pose::Cling(phase) => pen.transformed(CLING, |pen| stand(pen, Stance::walking(phase))),
        Pose::Ceiling(phase) => pen.transformed(CEILING, |pen| stand(pen, Stance::walking(phase))),
        Pose::Sleep if coat.is_pig() => pig::sleeping(pen, coat),
        Pose::Sleep => cat::sleeping(pen, coat),
        Pose::Dangle { swing, kick } => pen.transformed(swing_shear(swing), |pen| {
            if coat.is_pig() {
                pig::hanging(pen, coat, kicked(kick))
            } else {
                cat::hanging(pen, coat, kicked(kick))
            }
        }),
    }
}

/// Tips the animal `slope` rows per column about `BOW_PIVOT`: the front
/// sinks, the back rises, verticals stay vertical.
fn tip(slope: f64) -> CGAffineTransform {
    CGAffineTransform {
        a: 1.0,
        b: slope,
        c: 0.0,
        d: 1.0,
        tx: 0.0,
        ty: -slope * BOW_PIVOT,
    }
}

/// Rows lower down swing further, the scruff stays put: the lowest row
/// moves `swing` pixels, as in the sprite.
fn swing_shear(swing: i8) -> CGAffineTransform {
    let (_, top) = crate::sprites::SCRUFF;
    let per_row = f64::from(swing) / (CANVAS_H - 1 - top) as f64;
    CGAffineTransform {
        a: 1.0,
        b: 0.0,
        c: per_row,
        d: 1.0,
        tx: -per_row * top as f64,
        ty: 0.0,
    }
}

/// Which hind leg a kick pulls up, 0 for the left one.
fn kicked(kick: Kick) -> Option<usize> {
    match kick {
        Kick::None => None,
        Kick::Left => Some(0),
        Kick::Right => Some(1),
    }
}

#[cfg(test)]
mod tests;
