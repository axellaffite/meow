//! Now and then, and often on waking, the cat stretches: a bow, front legs
//! reaching ahead and rump in the air, or its back arched high. Walking
//! into the edge of the screen, it may stop to claw it before turning back.
//! Dédé the pig does neither.

use crate::sprites::{Facing, Pose};

use super::{Activity, Bounds, Cat, Rng};

/// Chance, when an idle cat picks what to do next, of stretching.
pub const STRETCH_CHANCE: f64 = 0.08;
/// Chance that a cat waking from a nap stretches before walking off.
pub const WAKE_STRETCH_CHANCE: f64 = 0.6;
/// Chance that a cat walking into a wall claws it.
pub const SCRATCH_CHANCE: f64 = 0.5;
pub const STRETCH_SECONDS: f64 = 3.0;
/// Easing into the stretch and out of it, at each end.
const EASE_SECONDS: f64 = 0.5;
pub const SCRATCH_SECONDS: f64 = 2.5;
const SCRATCH_FPS: f64 = 6.0;

/// Which way the cat stretches.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Stretch {
    /// Front legs reaching ahead, rump in the air.
    Bow,
    /// Back arched high.
    Arch,
}

impl Cat {
    /// Starts stretching, a bow or an arch, unless the cat is busy with
    /// something else. Returns whether it started.
    pub fn stretch_out(&mut self, rng: &mut Rng) -> bool {
        let free = matches!(
            self.activity,
            Activity::Walking | Activity::Idle | Activity::Sleeping
        );
        if !free || self.pig || self.has_prop() {
            return false;
        }
        self.stretch = if rng.next_f64() < 0.5 {
            Stretch::Bow
        } else {
            Stretch::Arch
        };
        self.activity = Activity::Stretching;
        self.stretch_for = 0.0;
        true
    }

    /// One tick of stretching; done, the cat stands a moment.
    pub(super) fn stretch(&mut self, dt: f64, rng: &mut Rng) {
        self.stretch_for += dt;
        if self.stretch_for >= STRETCH_SECONDS {
            self.activity = Activity::Idle;
            self.time_left = rng.range(1.0, 2.5);
        }
    }

    /// Eases in, holds the stretch, eases out.
    pub(super) fn stretch_pose(&self) -> Pose {
        let easing =
            self.stretch_for < EASE_SECONDS || self.stretch_for > STRETCH_SECONDS - EASE_SECONDS;
        let phase = u8::from(!easing);
        match self.stretch {
            Stretch::Bow => Pose::Bow(phase),
            Stretch::Arch => Pose::Arch(phase),
        }
    }

    /// Walking into a wall it was facing: maybe stops to claw it. Returns
    /// whether it did.
    pub(super) fn maybe_scratch(&mut self, wall: Facing, bounds: Bounds, rng: &mut Rng) -> bool {
        let open = match wall {
            Facing::Left => bounds.open.left,
            Facing::Right => bounds.open.right,
        };
        if open || self.pig || rng.next_f64() >= SCRATCH_CHANCE {
            return false;
        }
        self.facing = wall;
        self.activity = Activity::Scratching;
        self.stretch_for = 0.0;
        true
    }

    /// One tick of clawing the wall; done, the cat turns and walks off.
    pub(super) fn scratch(&mut self, dt: f64, rng: &mut Rng) {
        self.stretch_for += dt;
        if self.stretch_for < SCRATCH_SECONDS {
            return;
        }
        self.facing = match self.facing {
            Facing::Right => Facing::Left,
            Facing::Left => Facing::Right,
        };
        self.activity = Activity::Walking;
        self.time_left = rng.range(3.0, 8.0);
    }

    pub(super) fn scratch_pose(&self) -> Pose {
        Pose::Scratch((self.stretch_for * SCRATCH_FPS) as u64 as u8 % 4)
    }
}
