//! Now and then the cat coughs up a hairball, which stays on the floor until
//! the user wipes it away, or for 10 seconds at most. Dédé the pig poops instead, then turns around and
//! eats it, unless the user wipes it first.

use crate::sprites::{Bubble, Facing, MOUTH, Pose, Prop, SCALE, SPIT};

use super::play::{BITE_FPS, BOWL_OVERLAP, CAT_WIDTH, PROP_WIDTH};
use super::{Activity, Bounds, Cat, HEARTS_FPS};

/// Chance, when an idle cat picks what to do next, of coughing up a hairball.
pub const HAIRBALL_CHANCE: f64 = 0.02;
pub const HACK_SECONDS: f64 = 2.4;
/// The end of the hack, head down to the floor, bringing it up.
pub const SPIT_SECONDS: f64 = 0.4;
const HEAVE_FPS: f64 = 4.0;
/// The hairball lands this far past the front of the jaw.
const SPIT_REACH: f64 = 2.0 * SCALE;
/// Scrubbing (points of mouse travel) that wipes the hairball away.
pub const WIPE_DISTANCE: f64 = 300.0;
/// What one click on the hairball wipes.
pub const WIPE_PER_TAP: f64 = WIPE_DISTANCE / 4.0;
const DIRT_LEVELS: f64 = 3.0;
/// The spot glints this long once wiped clean.
pub const SPARKLE_SECONDS: f64 = 0.8;
const SPARKLE_FPS: f64 = 6.0;
/// The hairball goes away on its own after lying on the floor this long.
pub const LINGER_SECONDS: f64 = 10.0;
/// Dédé admires his poop this long, then eats it in this long.
pub const SNIFF_SECONDS: f64 = 1.2;
pub const GOBBLE_SECONDS: f64 = 4.0;

/// A hairball on the floor, where the cat left it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Hairball {
    /// Left edge of its window.
    x: f64,
    facing: Facing,
    /// Points of scrubbing so far.
    wiped: f64,
    /// Seconds since it was wiped clean, while the spot glints.
    clean_for: Option<f64>,
    /// Seconds since it landed.
    on_floor: f64,
    /// Dédé's poop rather than a hairball.
    poop: bool,
}

impl Cat {
    /// Dédé is a pig: from now on he poops instead of coughing up hairballs.
    pub fn set_pig(&mut self, pig: bool) {
        self.pig = pig;
    }

    /// Starts coughing up a hairball (or pooping, for Dédé), unless one is already on the floor or
    /// the cat is busy with something else. Returns whether it started.
    pub fn cough_up(&mut self) -> bool {
        let free = matches!(
            self.activity,
            Activity::Walking | Activity::Idle | Activity::Sleeping
        );
        if !free || self.hairball.is_some() || self.has_prop() {
            return false;
        }
        self.activity = Activity::Hacking;
        self.hack_left = HACK_SECONDS;
        true
    }

    /// One tick of hacking; at the end the hairball lands in front of the cat.
    pub(super) fn hack(&mut self, dt: f64, bounds: Bounds) {
        self.hack_left -= dt;
        if self.hack_left > 0.0 {
            return;
        }
        self.hack_left = 0.0;
        if self.pig {
            return self.drop_poop(bounds);
        }
        self.hairball = Some(Hairball {
            x: self.spit_spot(bounds),
            facing: self.facing,
            wiped: 0.0,
            clean_for: None,
            on_floor: 0.0,
            poop: false,
        });
        self.activity = Activity::Idle;
        self.time_left = 2.0;
    }

    /// The poop lands behind Dédé, right where his bowl would be once he
    /// turns around; and he turns around.
    fn drop_poop(&mut self, bounds: Bounds) {
        let behind = match self.facing {
            Facing::Right => Facing::Left,
            Facing::Left => Facing::Right,
        };
        let x = match behind {
            Facing::Right => self.x + CAT_WIDTH - BOWL_OVERLAP,
            Facing::Left => self.x - PROP_WIDTH + BOWL_OVERLAP,
        };
        self.hairball = Some(Hairball {
            x: x.clamp(bounds.min_x, bounds.max_x + CAT_WIDTH - PROP_WIDTH),
            facing: behind,
            wiped: 0.0,
            clean_for: None,
            on_floor: 0.0,
            poop: true,
        });
        self.facing = behind;
        self.activity = Activity::Gobbling;
        self.gobble_for = 0.0;
    }

    /// One tick of Dédé at his poop: he looks at it fondly, then eats it
    /// bite by bite. Wiped away under his nose, he gives up.
    pub(super) fn gobble(&mut self, dt: f64) {
        self.gobble_for += dt;
        let eating = self.gobble_for - SNIFF_SECONDS;
        let Some(poop) = self.hairball.as_mut().filter(|h| h.clean_for.is_none()) else {
            self.activity = Activity::Idle;
            self.time_left = 2.0;
            return;
        };
        if eating <= 0.0 {
            return;
        }
        // What he eats counts like scrubbing, so the poop shrinks the same way.
        poop.wiped = poop.wiped.max(eating / GOBBLE_SECONDS * WIPE_DISTANCE);
        if poop.wiped >= WIPE_DISTANCE {
            // Eaten: nothing left to glint.
            self.hairball = None;
            self.activity = Activity::Idle;
            self.time_left = 2.0;
        }
    }

    /// Sniffing his poop, eyes shut with delight, then head down in it.
    pub(super) fn gobble_pose(&self) -> Pose {
        if self.gobble_for < SNIFF_SECONDS {
            return Pose::Blink;
        }
        Pose::Eat(((self.gobble_for - SNIFF_SECONDS) * BITE_FPS) as u64 as u8)
    }

    /// Hearts while Dédé admires his poop.
    pub(super) fn gobble_bubble(&self) -> Option<Bubble> {
        let admiring = self.activity == Activity::Gobbling && self.gobble_for < SNIFF_SECONDS;
        admiring.then_some(Bubble::Hearts((self.gobble_for * HEARTS_FPS) as u64 as u8))
    }

    /// Left edge of the hairball's window: just past the jaw, on the screen.
    fn spit_spot(&self, bounds: Bounds) -> f64 {
        let (mouth_x, _) = self.anchor(MOUTH);
        let x = match self.facing {
            Facing::Right => mouth_x - SPIT_REACH,
            Facing::Left => mouth_x + SPIT_REACH - PROP_WIDTH,
        };
        x.clamp(bounds.min_x, bounds.max_x + CAT_WIDTH - PROP_WIDTH)
    }

    pub(super) fn hack_pose(&self) -> Pose {
        let pose = if self.pig { Pose::Squat } else { Pose::Hack };
        if self.hack_left <= SPIT_SECONDS {
            return pose(SPIT);
        }
        let heaves = (HACK_SECONDS - self.hack_left) * HEAVE_FPS;
        pose(heaves as u64 as u8 % 2)
    }

    /// The spot glints for a moment once wiped clean, then is gone; left
    /// alone, the hairball is gone after `LINGER_SECONDS`.
    pub(super) fn age_hairball(&mut self, dt: f64) {
        let Some(hairball) = &mut self.hairball else {
            return;
        };
        hairball.on_floor += dt;
        let Some(clean_for) = &mut hairball.clean_for else {
            if hairball.on_floor >= LINGER_SECONDS {
                self.hairball = None;
            }
            return;
        };
        *clean_for += dt;
        if *clean_for >= SPARKLE_SECONDS {
            self.hairball = None;
        }
    }

    /// The user scrubbed `distance` points over the hairball.
    pub fn wipe_hairball(&mut self, distance: f64) {
        let Some(hairball) = &mut self.hairball else {
            return;
        };
        if hairball.clean_for.is_some() {
            return;
        }
        hairball.wiped += distance.abs();
        if hairball.wiped >= WIPE_DISTANCE {
            hairball.clean_for = Some(0.0);
        }
    }

    /// The user clicked the hairball.
    pub fn tap_hairball(&mut self) {
        self.wipe_hairball(WIPE_PER_TAP);
    }

    /// The hairball on the floor, or the glint where it was.
    pub fn hairball(&self) -> Option<super::PropView> {
        let hairball = self.hairball?;
        let prop = match hairball.clean_for {
            Some(clean_for) => Prop::Sparkle((clean_for * SPARKLE_FPS) as u64 as u8),
            None => {
                let left = 1.0 - hairball.wiped / WIPE_DISTANCE;
                let left = (left * DIRT_LEVELS).ceil().clamp(1.0, DIRT_LEVELS) as u8;
                if hairball.poop {
                    Prop::Poop(left)
                } else {
                    Prop::Hairball(left)
                }
            }
        };
        Some(super::PropView {
            prop,
            x: hairball.x.round() as i64,
            facing: hairball.facing,
        })
    }
}
