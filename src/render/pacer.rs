//! When a window redraws its creature: on any change of the look, on any
//! change of the frame (at most [`BATTERY_FPS`] on Battery: the breathing
//! never stops), at the quality's idle rate while it is calm, and otherwise
//! not at all. A creature standing still costs no GPU.

use glam::{Mat4, Vec2};

use super::{Quality, Theme};
use crate::creature::frame::Frame;

/// The most frames a second Battery draws for motion (the tick is 30 Hz).
pub const BATTERY_FPS: f32 = 15.0;

/// How early a frame may come against its rate: half a 30 Hz tick, as an
/// `NSTimer` fires a few milliseconds early or late. Every rate here (30,
/// 15, 6) is a whole number of ticks, so the tick that is due draws, and
/// never the one before it.
const TICK_SLACK: f32 = 0.5 / 30.0;

/// What makes a redraw necessary besides the frame itself.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Canvas {
    pub theme: Theme,
    pub quality: Quality,
    /// The draw offset, points.
    pub shift: Vec2,
    /// Drawable size in pixels.
    pub pixels: (usize, usize),
    /// The camera's pitch, degrees.
    pub pitch: f32,
}

#[derive(Debug, Default)]
pub struct Pacer {
    last: Option<(Frame, Canvas, f32)>,
}

impl Pacer {
    /// Whether to draw `frame` with `look` at `time` (seconds), given whether
    /// the creature is `calm` (asleep, nothing happening: the caller knows).
    pub fn should_draw(&self, frame: &Frame, look: &Canvas, time: f32, calm: bool) -> bool {
        let Some((last_frame, last_look, last_time)) = &self.last else {
            return true;
        };
        if last_look != look {
            return true;
        }
        let elapsed = time - last_time;
        // The clock wrapped or jumped: draw.
        if !(0.0..=10.0).contains(&elapsed) {
            return true;
        }
        if same_motion(last_frame, frame) {
            return false;
        }
        let fps = if calm {
            look.quality.idle_fps().min(BATTERY_FPS)
        } else {
            match look.quality {
                Quality::Battery => BATTERY_FPS,
                Quality::Balanced => return true,
            }
        };
        elapsed >= 1.0 / fps - TICK_SLACK
    }

    pub fn drew(&mut self, frame: &Frame, look: &Canvas, time: f32) {
        // Into the frame kept last time, reusing its buffers.
        match &mut self.last {
            Some((kept, kept_look, kept_time)) => {
                kept.copy_from(frame);
                *kept_look = *look;
                *kept_time = time;
            }
            None => self.last = Some((frame.clone(), *look, time)),
        }
    }

    /// Forgets what was drawn: the next frame draws.
    pub fn reset(&mut self) {
        self.last = None;
    }
}

/// Whether two frames give the same picture, up to what no one can see
/// (a hundredth of a millimetre of motion).
fn same_motion(a: &Frame, b: &Frame) -> bool {
    let close = |x: &Mat4, y: &Mat4| x.abs_diff_eq(*y, 1e-5);
    a.skin.len() == b.skin.len()
        && a.skin.iter().zip(&b.skin).all(|(x, y)| close(x, y))
        && a.model.abs_diff_eq(b.model, 1e-3)
        && a.shadow == b.shadow
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature::CreatureAsset;
    use crate::motion::{Animator, Placement, Play};
    use glam::Vec3;

    const FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/mini_creature.glb");

    fn animator() -> Animator {
        let asset = CreatureAsset::from_glb(FIXTURE).unwrap();
        let mut animator = Animator::new(&asset);
        animator.play("idle", Play::Loop, 1.0, 0.0);
        animator
    }

    fn placement() -> Placement {
        Placement::new(Vec2::new(90.0, 20.0), 150.0)
    }

    fn frame() -> Frame {
        animator().update(1.0 / 30.0, &placement())
    }

    fn canvas(quality: Quality) -> Canvas {
        Canvas {
            theme: Theme::Light,
            quality,
            shift: Vec2::ZERO,
            pixels: (360, 450),
            pitch: 6.0,
        }
    }

    /// `frame` with its second joint moved `mm` millimetres up.
    fn moved(frame: &Frame, mm: f32) -> Frame {
        let mut moved = frame.clone();
        moved.skin[1] *= Mat4::from_translation(Vec3::Y * mm / 1000.0);
        moved
    }

    #[test]
    fn a_still_creature_is_drawn_once() {
        let (frame, look) = (frame(), canvas(Quality::Balanced));
        let mut pacer = Pacer::default();
        assert!(pacer.should_draw(&frame, &look, 0.0, false));
        pacer.drew(&frame, &look, 0.0);
        for tick in 1..60 {
            assert!(!pacer.should_draw(&frame, &look, tick as f32 / 30.0, false));
        }
    }

    #[test]
    fn any_change_redraws() {
        let (frame, look) = (frame(), canvas(Quality::Balanced));
        let mut pacer = Pacer::default();
        pacer.drew(&frame, &look, 0.0);
        let dark = Canvas {
            theme: Theme::Dark,
            ..look
        };
        assert!(pacer.should_draw(&frame, &dark, 0.1, false), "theme");
        let shifted = Canvas {
            shift: Vec2::new(0.0, 3.0),
            ..look
        };
        assert!(pacer.should_draw(&frame, &shifted, 0.1, false), "shift");
        let retina = Canvas {
            pixels: (720, 900),
            ..look
        };
        assert!(pacer.should_draw(&frame, &retina, 0.1, false), "backing");
        let pitched = Canvas {
            pitch: 12.0,
            ..look
        };
        assert!(pacer.should_draw(&frame, &pitched, 0.1, false), "pitch");
        assert!(
            pacer.should_draw(&moved(&frame, 1.0), &look, 0.1, false),
            "pose"
        );
        let mut placed = frame.clone();
        placed.model *= Mat4::from_translation(Vec3::X);
        assert!(pacer.should_draw(&placed, &look, 0.1, false), "place");
        let mut shadowless = frame.clone();
        shadowless.shadow = None;
        assert!(pacer.should_draw(&shadowless, &look, 0.1, false), "shadow");
        pacer.reset();
        assert!(pacer.should_draw(&frame, &look, 0.1, false), "forgotten");
    }

    #[test]
    fn a_timer_firing_early_or_late_keeps_the_rate() {
        // A 30 Hz `NSTimer` fires a few milliseconds off: each rate still
        // draws on every tick due, and on none before.
        let jitter = [0.0, 0.004, -0.003, 0.006, -0.005, 0.002, -0.006, 0.005];
        let base = frame();
        for (quality, calm, expected) in [
            (Quality::Battery, false, 30i32),
            (Quality::Balanced, false, 60),
            (Quality::Balanced, true, 30),
            (Quality::Battery, true, 12),
        ] {
            let look = canvas(quality);
            let mut pacer = Pacer::default();
            let mut drawn = 0;
            for tick in 0..60 {
                let time = tick as f32 / 30.0 + jitter[tick % jitter.len()];
                // A new pose each tick, 4 mm on.
                let shown = moved(&base, 4.0 * tick as f32);
                if pacer.should_draw(&shown, &look, time, calm) {
                    pacer.drew(&shown, &look, time);
                    drawn += 1;
                }
            }
            assert_eq!(
                drawn, expected,
                "{quality:?} (calm {calm}): {drawn} frames in 2 s"
            );
        }
    }

    #[test]
    fn battery_draws_motion_at_15_fps() {
        let mut animator = animator();
        for (quality, expected) in [(Quality::Battery, 30i32), (Quality::Balanced, 60)] {
            let look = canvas(quality);
            let mut pacer = Pacer::default();
            let mut drawn = 0;
            let mut frame = animator.update(1.0 / 30.0, &placement());
            for tick in 0..60 {
                // It walks across its window.
                let mut placed = placement();
                placed.origin.x += tick as f32;
                animator.update_into(1.0 / 30.0, &placed, &mut frame);
                let time = tick as f32 / 30.0;
                if pacer.should_draw(&frame, &look, time, false) {
                    pacer.drew(&frame, &look, time);
                    drawn += 1;
                }
            }
            assert!(
                (drawn - expected).abs() <= 1,
                "{quality:?}: {drawn} frames in 2 s, {expected} expected"
            );
        }
    }

    #[test]
    fn a_frame_copied_into_another_is_the_same() {
        let mut animator = animator();
        let mut into = animator.update(1.0 / 30.0, &placement());
        animator.play("nod", Play::Loop, 1.0, 0.0);
        let mut placed = placement();
        placed.mirror = true;
        let from = animator.update(0.5, &placed);
        assert_ne!(into, from);
        into.copy_from(&from);
        assert_eq!(into, from);
    }
}
