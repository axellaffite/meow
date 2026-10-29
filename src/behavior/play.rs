//! Playtime: eating from a bowl, chasing a mouse across the screen, and
//! hunting the mouse pointer when it is shaken nearby.

use crate::sprites::{CANVAS_H, CANVAS_W, Facing, MOUTH, PROP_W, Pose, Prop, SCALE, SWAT_PAW};

use super::{Activity, Bounds, Cat, GRAVITY, Rng, WALK_FPS};

/// The cat window's width, and a prop window's, in points.
pub const CAT_WIDTH: f64 = CANVAS_W as f64 * SCALE;
pub const PROP_WIDTH: f64 = PROP_W as f64 * SCALE;
/// The bowl tucks this far under the cat's lowered head.
pub(super) const BOWL_OVERLAP: f64 = 2.0 * SCALE;
pub const MEAL_SECONDS: f64 = 8.0;
pub(super) const BITE_FPS: f64 = 4.0;
const KIBBLE_LEVELS: f64 = 3.0;

/// The mouse outruns the cat, so it always gets away.
pub const PREY_SPEED: f64 = 240.0;
pub const CHASE_SPEED: f64 = 170.0;
/// The cat freezes with a `!` this long before giving chase.
pub const STARTLE_SECONDS: f64 = 0.5;
const RUN_FPS: f64 = WALK_FPS * 2.5;
const MOUSE_FPS: f64 = 12.0;

/// Pointer shakes count within this distance of the cat (points).
pub const NEAR_X: f64 = 250.0;
pub const NEAR_Y: f64 = 250.0;
/// The cat keeps hunting this long after the last shake.
pub const HUNT_LINGER: f64 = 2.5;
const HUNT_SPEED: f64 = 120.0;
/// Close enough to swat at the pointer instead of running to it.
pub const SWAT_REACH: f64 = 45.0;
const SWAT_FPS: f64 = 5.0;
/// A pointer this high above the ground, within reach, gets pounced on.
pub const POUNCE_MIN_HEIGHT: f64 = 60.0;
const POUNCE_MAX_HEIGHT: f64 = 250.0;
/// Under the speed that would make the cat cling to a wall.
const POUNCE_MAX_SPEED: f64 = 250.0;
const POUNCE_COOLDOWN: f64 = 1.2;
/// A low pointer this close to the paw (points) can be caught.
pub const CATCH_REACH: f64 = 30.0;
pub const CATCH_HEIGHT: f64 = 50.0;
/// Chance per second, while swatting at a pointer within reach, of catching it.
pub const CATCH_RATE: f64 = 3.0;
/// Chance that a pounce which reaches the pointer snatches it.
pub const POUNCE_CATCH_CHANCE: f64 = 0.75;
/// The caught pointer is held still this long, give or take.
pub const HOLD_MIN: f64 = 1.5;
pub const HOLD_MAX: f64 = 2.5;
/// The caught pointer stays pinned under the paw this long, then the cat
/// trots off with it in its mouth.
pub const PIN_SECONDS: f64 = 0.5;
pub const CARRY_SPEED: f64 = 80.0;
const CARRY_FPS: f64 = WALK_FPS * 1.5;
/// After letting go, the cat ignores shakes for a while.
pub const CATCH_COOLDOWN: f64 = 4.0;

/// The mouse running across the screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Prey {
    /// Left edge of its window.
    pub x: f64,
    pub heading: Facing,
    pub age: f64,
}

/// A prop to show next to the cat, with its window's left edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PropView {
    pub prop: Prop,
    pub x: i64,
    pub facing: Facing,
}

fn toward(dx: f64) -> Facing {
    if dx >= 0.0 {
        Facing::Right
    } else {
        Facing::Left
    }
}

impl Cat {
    fn center(&self) -> f64 {
        self.x + CAT_WIDTH / 2.0
    }

    pub(super) fn has_prop(&self) -> bool {
        self.prey.is_some() || self.activity == Activity::Eating
    }

    pub(super) fn is_grounded(&self) -> bool {
        matches!(
            self.activity,
            Activity::Walking
                | Activity::Idle
                | Activity::Sleeping
                | Activity::Eating
                | Activity::Chasing
                | Activity::Hunting
                | Activity::Hacking
                | Activity::Gobbling
                | Activity::Stretching
                | Activity::Scratching
        )
    }

    /// Puts down a bowl of kibble on whichever side has room. Returns whether
    /// the cat started eating.
    pub fn feed(&mut self, bounds: Bounds) -> bool {
        if !self.is_grounded() || self.has_prop() {
            return false;
        }
        let room = |facing| match facing {
            Facing::Right => self.x + PROP_WIDTH - BOWL_OVERLAP <= bounds.max_x,
            Facing::Left => self.x - PROP_WIDTH + BOWL_OVERLAP >= bounds.min_x,
        };
        let other = match self.facing {
            Facing::Right => Facing::Left,
            Facing::Left => Facing::Right,
        };
        let Some(facing) = [self.facing, other].into_iter().find(|&f| room(f)) else {
            return false;
        };
        self.facing = facing;
        self.activity = Activity::Eating;
        self.meal_left = MEAL_SECONDS;
        true
    }

    /// Lets a mouse in from a random side of the screen; the cat gives chase.
    pub fn release_mouse(&mut self, bounds: Bounds, rng: &mut Rng) -> bool {
        if !self.is_grounded() || self.has_prop() {
            return false;
        }
        let heading = if rng.next_f64() < 0.5 {
            Facing::Left
        } else {
            Facing::Right
        };
        let x = match heading {
            Facing::Right => bounds.min_x - PROP_WIDTH,
            Facing::Left => bounds.max_x + CAT_WIDTH,
        };
        self.prey = Some(Prey {
            x,
            heading,
            age: 0.0,
        });
        self.activity = Activity::Chasing;
        self.chase_for = 0.0;
        true
    }

    /// The mouse runs on, whatever the cat does, until it leaves the screen.
    pub(super) fn run_prey(&mut self, dt: f64, bounds: Bounds) {
        let Some(prey) = &mut self.prey else { return };
        prey.age += dt;
        prey.x += match prey.heading {
            Facing::Right => PREY_SPEED * dt,
            Facing::Left => -PREY_SPEED * dt,
        };
        let gone = match prey.heading {
            Facing::Right => prey.x > bounds.max_x + CAT_WIDTH,
            Facing::Left => prey.x < bounds.min_x - PROP_WIDTH,
        };
        if gone {
            self.prey = None;
        }
    }

    /// Where the pointer is, relative to the ground; called every tick.
    pub fn see_pointer(&mut self, x: f64, y: f64) {
        self.pointer = Some((x, y));
        self.shake.add(self.clock, x);
    }

    /// A shake of the pointer near a cat on the ground sets it hunting.
    pub(super) fn notice_shake(&mut self) {
        let Some((x, y)) = self.pointer else { return };
        let near = (x - self.center()).abs() <= NEAR_X && (0.0..=NEAR_Y).contains(&y);
        if !near || self.catch_cooldown > 0.0 || !self.shake.is_shaking() {
            return;
        }
        self.hunt_left = HUNT_LINGER;
        self.affection = 0.0;
        self.petted_left = 0.0;
        if !matches!(self.activity, Activity::Eating | Activity::Chasing) {
            self.activity = Activity::Hunting;
        }
    }

    /// One tick of eating; the bowl goes once it is empty.
    pub(super) fn eat(&mut self, dt: f64) {
        self.meal_left -= dt;
        if self.meal_left <= 0.0 {
            self.meal_left = 0.0;
            self.activity = Activity::Idle;
            self.time_left = 2.0;
        }
    }

    /// After a startled moment, runs after the mouse until it is gone.
    pub(super) fn chase(&mut self, dt: f64, bounds: Bounds, rng: &mut Rng) {
        self.chase_for += dt;
        let Some(prey) = self.prey else {
            self.running = false;
            self.activity = Activity::Idle;
            self.time_left = rng.range(2.0, 4.0);
            return;
        };
        let dx = prey.x + PROP_WIDTH / 2.0 - self.center();
        self.facing = toward(dx);
        self.running = self.chase_for >= STARTLE_SECONDS;
        if self.running {
            self.run_toward(dx, CHASE_SPEED * dt, bounds);
        }
    }

    /// Runs to the pointer, swats at it when close, pounces when it hovers above.
    pub(super) fn hunt(&mut self, dt: f64, bounds: Bounds, rng: &mut Rng) {
        if self.holding_for > 0.0 {
            return self.hold(dt, bounds);
        }
        self.hunt_left -= dt;
        self.pounce_cooldown = (self.pounce_cooldown - dt).max(0.0);
        let Some((x, y)) = self.pointer else { return };
        if self.hunt_left <= 0.0 {
            self.running = false;
            self.activity = Activity::Idle;
            self.time_left = 1.5;
            return;
        }
        let dx = x - self.center();
        self.facing = toward(dx);
        self.running = dx.abs() > SWAT_REACH;
        if self.running {
            self.run_toward(dx, HUNT_SPEED * dt, bounds);
        } else if y >= POUNCE_MIN_HEIGHT && self.pounce_cooldown <= 0.0 {
            self.pounce(x, y, rng);
        } else if self.within_paw(x, y) && rng.next_f64() < CATCH_RATE * dt {
            self.holding_for = rng.range(HOLD_MIN, HOLD_MAX);
            self.caught_for = 0.0;
        }
    }

    fn within_paw(&self, x: f64, y: f64) -> bool {
        let (paw_x, _) = self.paw();
        (x - paw_x).abs() <= CATCH_REACH && y <= CATCH_HEIGHT
    }

    /// Pins the pointer under the paw, carries it off in its mouth, then
    /// lets go, satisfied.
    fn hold(&mut self, dt: f64, bounds: Bounds) {
        self.holding_for -= dt;
        self.caught_for += dt;
        self.running = self.is_carrying();
        if self.running {
            let step = CARRY_SPEED * dt;
            self.x += match self.facing {
                Facing::Right => step,
                Facing::Left => -step,
            };
            // Turns at the screen edge, prize and all.
            if self.x >= bounds.max_x || self.x <= bounds.min_x {
                self.x = self.x.clamp(bounds.min_x, bounds.max_x);
                self.facing = toward(bounds.min_x + bounds.max_x - 2.0 * self.x);
            }
        }
        if self.holding_for > 0.0 {
            return;
        }
        self.running = false;
        self.holding_for = 0.0;
        self.hunt_left = 0.0;
        self.catch_cooldown = CATCH_COOLDOWN;
        self.activity = Activity::Idle;
        self.time_left = 1.5;
    }

    fn is_carrying(&self) -> bool {
        self.holding_for > 0.0 && self.caught_for >= PIN_SECONDS
    }

    /// Where the tip of the swatting paw is, in points (`y` above the ground).
    fn paw(&self) -> (f64, f64) {
        self.anchor(SWAT_PAW)
    }

    /// Where a canvas pixel of the cat is, in points (`y` above the ground).
    pub(super) fn anchor(&self, (col, row): (usize, usize)) -> (f64, f64) {
        let col = match self.facing {
            Facing::Right => col as f64 + 0.5,
            Facing::Left => (CANVAS_W - col) as f64 - 0.5,
        };
        (
            self.x + col * SCALE,
            self.y + (CANVAS_H - row) as f64 * SCALE - SCALE / 2.0,
        )
    }

    /// The caught pointer's spot, under the paw then in the mouth, while the
    /// cat has it.
    pub fn caught_pointer(&self) -> Option<(f64, f64)> {
        let active = matches!(self.activity, Activity::Hunting | Activity::Falling);
        if !active || self.holding_for <= 0.0 {
            return None;
        }
        Some(if self.is_carrying() {
            self.anchor(MOUTH)
        } else {
            self.paw()
        })
    }

    /// Steps up to `step` toward `dx`, without overshooting or leaving the screen.
    fn run_toward(&mut self, dx: f64, step: f64, bounds: Bounds) {
        self.x = (self.x + dx.clamp(-step, step)).clamp(bounds.min_x, bounds.max_x);
    }

    /// Leaps so its mouth meets the pointer at the top of the jump.
    fn pounce(&mut self, x: f64, y: f64, rng: &mut Rng) {
        let (mouth_x, mouth_y) = self.anchor(MOUTH);
        let height = (y - mouth_y).clamp(SCALE, POUNCE_MAX_HEIGHT);
        self.vy = (2.0 * GRAVITY * height).sqrt();
        let rise = self.vy / GRAVITY;
        self.vx = ((x - mouth_x) / rise).clamp(-POUNCE_MAX_SPEED, POUNCE_MAX_SPEED);
        self.running = false;
        self.pounce_cooldown = POUNCE_COOLDOWN;
        self.pouncing = true;
        self.pounce_hits = rng.next_f64() < POUNCE_CATCH_CHANCE;
        self.activity = Activity::Falling;
    }

    /// Mid-pounce, a mouth that reaches the pointer may snatch it; the cat
    /// lands with it and trots off.
    pub(super) fn snatch_in_flight(&mut self, rng: &mut Rng) {
        if self.activity != Activity::Falling {
            self.pouncing = false;
            if self.holding_for > 0.0 {
                // Landed with it: straight on, without letting go for a tick.
                self.activity = Activity::Hunting;
            }
            return;
        }
        let Some((x, y)) = self.pointer else { return };
        if !self.pouncing || self.holding_for > 0.0 {
            return;
        }
        let (mouth_x, mouth_y) = self.anchor(MOUTH);
        if (x - mouth_x).hypot(y - mouth_y) > CATCH_REACH {
            return;
        }
        self.pouncing = false;
        if self.pounce_hits {
            self.holding_for = rng.range(HOLD_MIN, HOLD_MAX);
            // Already in its mouth: no pinning down.
            self.caught_for = PIN_SECONDS;
        }
    }

    /// Pose for the play activities; `None` for the others.
    pub(super) fn play_pose(&self) -> Option<Pose> {
        let frame = |fps: f64| (self.clock * fps) as u64 as u8;
        match self.activity {
            Activity::Eating => Some(Pose::Eat(frame(BITE_FPS))),
            // Paw pressed down on the caught pointer.
            Activity::Hunting if self.is_carrying() => Some(Pose::Walk(frame(CARRY_FPS) % 4)),
            Activity::Hunting if self.holding_for > 0.0 => Some(Pose::Swat(1)),
            Activity::Chasing | Activity::Hunting if self.running => {
                Some(Pose::Walk(frame(RUN_FPS) % 4))
            }
            Activity::Hunting => Some(Pose::Swat(frame(SWAT_FPS))),
            Activity::Chasing => Some(Pose::Stand),
            _ => None,
        }
    }

    pub fn is_startled(&self) -> bool {
        self.activity == Activity::Chasing && self.chase_for < STARTLE_SECONDS
    }

    /// The bowl while eating, or the running mouse.
    pub fn prop(&self) -> Option<PropView> {
        if let Some(prey) = self.prey {
            return Some(PropView {
                prop: Prop::Mouse((prey.age * MOUSE_FPS) as u64 as u8),
                x: prey.x.round() as i64,
                facing: prey.heading,
            });
        }
        if self.activity != Activity::Eating {
            return None;
        }
        let level = (self.meal_left / MEAL_SECONDS * KIBBLE_LEVELS).ceil() as u8;
        let x = match self.facing {
            Facing::Right => self.x + CAT_WIDTH - BOWL_OVERLAP,
            Facing::Left => self.x - PROP_WIDTH + BOWL_OVERLAP,
        };
        Some(PropView {
            prop: Prop::Bowl(level),
            x: x.round() as i64,
            facing: self.facing,
        })
    }
}
