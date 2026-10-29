//! What the cat is doing on screen: wandering, napping, waiting on Claude.
//!
//! Pure logic, independent of AppKit, driven by `update` once per tick.

mod hairball;
mod play;
mod shake;
mod stretch;

pub use hairball::HAIRBALL_CHANCE;
pub use play::{CAT_WIDTH, PROP_WIDTH, Prey, PropView};
pub use stretch::{SCRATCH_CHANCE, STRETCH_CHANCE, Stretch, WAKE_STRETCH_CHANCE};

use crate::sprites::{Bubble, Facing, Kick, Look, Pose};
use hairball::Hairball;
use shake::Shake;

pub const WALK_SPEED: f64 = 28.0;
const WALK_FPS: f64 = 8.0;
const DOTS_FPS: f64 = 3.0;
const BLINK_PERIOD: f64 = 4.0;
const BLINK_LENGTH: f64 = 0.15;
/// Downward acceleration while falling, in points per second².
pub const GRAVITY: f64 = 2600.0;
const MAX_THROW_SPEED: f64 = 2500.0;
/// Share of horizontal speed kept when bouncing off a screen edge.
const WALL_BOUNCE: f64 = 0.4;
/// How fast the throw velocity follows the drag (0..1 per tick).
const DRAG_SMOOTHING: f64 = 0.5;
const FACE_THROW_SPEED: f64 = 50.0;
/// Drag speed (points/s) that makes the held cat trail one pixel behind.
const LEAN_SPEED: f64 = 250.0;
/// Widest swing of the held cat, in sprite pixels at its feet.
pub const MAX_SWING: f64 = 3.0;
/// Pendulum spring and damping: about one swing per second, settling in ~1 s.
const SWING_STIFFNESS: f64 = 60.0;
const SWING_DAMPING: f64 = 4.0;
/// A freshly grabbed cat kicks its hind legs this long, at this rate.
const KICK_SECONDS: f64 = 0.8;
const KICK_FPS: f64 = 6.0;
/// A throw this fast (points/s) sticks the cat to the screen edge instead of bouncing.
pub const CLING_MIN_SPEED: f64 = 300.0;
/// Below this height the cat just lands rather than grabbing the wall.
pub const CLING_MIN_HEIGHT: f64 = 40.0;
/// Paws scrabble against the wall for this long, then stay put.
const SCRABBLE_SECONDS: f64 = 0.5;
/// Push off the wall: a small hop away and up before falling.
const JUMP_OFF_SPEED: f64 = 160.0;
const JUMP_OFF_LIFT: f64 = 260.0;
/// Chance that a cat on a side wall slides down it instead of jumping off.
pub const SLIDE_CHANCE: f64 = 0.35;
/// Sliding down a wall: starts slow and speeds up, claws scraping.
const SLIDE_START_SPEED: f64 = 40.0;
const SLIDE_ACCELERATION: f64 = 160.0;
const SLIDE_MAX_SPEED: f64 = 320.0;
/// Mouse travel over the cat (points) that counts as petting it.
pub const PET_THRESHOLD: f64 = 120.0;
/// Stroking fades this fast (points/s) when the mouse stops.
const AFFECTION_DECAY: f64 = 200.0;
/// The cat stays happy this long after the last stroke.
pub const PET_LINGER: f64 = 1.5;
const HEARTS_FPS: f64 = 8.0;
const ZZZ_FPS: f64 = 3.0;
/// Chances, when an idle cat picks what to do next, of a snack or a mouse.
pub const SNACK_CHANCE: f64 = 0.06;
pub const MOUSE_CHANCE: f64 = 0.06;

/// What the cat is holding on to after a throw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClingSide {
    /// The left or right screen edge, whichever it faces.
    Wall,
    /// The top of the screen, upside down.
    Ceiling,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activity {
    Walking,
    Idle,
    Sleeping,
    /// Picked up with the mouse.
    Held,
    /// Dropped or thrown, on its way down.
    Falling,
    /// Stuck to the screen edge with its paws, after a throw.
    Clinging,
    /// Head in a bowl of kibble.
    Eating,
    /// After a mouse running across the screen.
    Chasing,
    /// After the mouse pointer, which was shaken nearby.
    Hunting,
    /// Hunched over, coughing up a hairball (or Dédé squatting to poop).
    Hacking,
    /// Dédé, turned around to his poop: sniffing it, then eating it.
    Gobbling,
    /// Stretching: a bow or an arched back.
    Stretching,
    /// Reared up against the screen edge, clawing it.
    Scratching,
}

/// Where the cat may go: `x` is the window's left edge, `y` its height above the ground.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub min_x: f64,
    pub max_x: f64,
    pub max_y: f64,
    pub open: OpenEdges,
}

/// The edges of the screen that lead on to another screen, at the cat's
/// height: a thrown or carried cat goes through them instead of hitting a wall.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OpenEdges {
    pub left: bool,
    pub right: bool,
    pub top: bool,
}

impl OpenEdges {
    /// A screen on its own: walls all around.
    pub const NONE: Self = Self {
        left: false,
        right: false,
        top: false,
    };
}

impl Bounds {
    pub fn new(min_x: f64, max_x: f64, max_y: f64) -> Self {
        Self {
            min_x,
            max_x: max_x.max(min_x),
            max_y: max_y.max(0.0),
            open: OpenEdges::NONE,
        }
    }

    /// `x` kept off the side edges that are walls.
    fn between_walls(&self, x: f64) -> f64 {
        let x = if self.open.left { x } else { x.max(self.min_x) };
        if self.open.right {
            x
        } else {
            x.min(self.max_x)
        }
    }

    /// `y` kept off the ground, and under the top edge if it is a wall.
    fn under_ceiling(&self, y: f64) -> f64 {
        let y = y.max(0.0);
        if self.open.top { y } else { y.min(self.max_y) }
    }
}

/// Outside state the cat reacts to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Inputs {
    /// Claude is working on a request.
    pub busy: bool,
    /// The chat window is open, so the cat stays put next to it.
    pub chat_open: bool,
    /// A reply arrived while the chat was closed.
    pub unread: bool,
    /// Activity has unread news: the cat shows it, but may still wander.
    pub news: bool,
}

impl Inputs {
    fn holds_cat(self) -> bool {
        self.busy || self.chat_open || self.unread
    }
}

/// Small xorshift generator, plenty for picking wander durations.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }

    pub fn next_f64(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }

    pub fn range(&mut self, min: f64, max: f64) -> f64 {
        min + (max - min) * self.next_f64()
    }
}

pub struct Cat {
    pub x: f64,
    /// Height above the ground.
    pub y: f64,
    pub facing: Facing,
    vx: f64,
    vy: f64,
    /// Last drag position seen by `update`, to estimate the throw velocity.
    drag_seen: (f64, f64),
    /// Sideways swing of the held cat (sprite pixels at its feet) and its speed.
    swing: f64,
    swing_v: f64,
    held_for: f64,
    /// Time since the cat grabbed the wall, and which one.
    clinging_for: f64,
    cling_side: ClingSide,
    /// This time the cat will slide down the wall rather than jump off.
    cling_slides: bool,
    /// Current speed down the wall; zero until the slide starts.
    slide_speed: f64,
    /// Recent stroking, in points of mouse travel; decays over time.
    affection: f64,
    /// Seconds of happiness (and hearts) left after being petted.
    petted_left: f64,
    /// Seconds of eating left; the bowl empties as it runs down.
    meal_left: f64,
    prey: Option<Prey>,
    chase_for: f64,
    /// Last pointer position, relative to the ground, and its recent moves.
    pointer: Option<(f64, f64)>,
    shake: Shake,
    /// Seconds of hunting the pointer left, refreshed by each shake.
    hunt_left: f64,
    pounce_cooldown: f64,
    /// Chasing or hunting at a run, rather than standing.
    running: bool,
    /// Mid-pounce at the pointer, and whether this pounce will snatch it.
    pouncing: bool,
    pounce_hits: bool,
    /// Seconds left holding the caught pointer, and since it was caught.
    holding_for: f64,
    caught_for: f64,
    /// Seconds left ignoring pointer shakes, after a catch.
    catch_cooldown: f64,
    /// Seconds of hacking left before the hairball comes up.
    hack_left: f64,
    /// Left on the floor until the user wipes it away.
    hairball: Option<Hairball>,
    /// Dédé the pig: he poops instead of coughing up hairballs.
    pig: bool,
    /// Seconds since Dédé turned around to his poop.
    gobble_for: f64,
    /// How the cat stretches this time, and for how long it has been
    /// stretching or clawing the wall.
    stretch: Stretch,
    stretch_for: f64,
    activity: Activity,
    time_left: f64,
    clock: f64,
}

impl Cat {
    pub fn new(x: f64) -> Self {
        Self {
            x,
            y: 0.0,
            facing: Facing::Right,
            vx: 0.0,
            vy: 0.0,
            drag_seen: (x, 0.0),
            swing: 0.0,
            swing_v: 0.0,
            held_for: 0.0,
            clinging_for: 0.0,
            cling_side: ClingSide::Wall,
            cling_slides: false,
            slide_speed: 0.0,
            affection: 0.0,
            petted_left: 0.0,
            meal_left: 0.0,
            prey: None,
            chase_for: 0.0,
            pointer: None,
            shake: Shake::default(),
            hunt_left: 0.0,
            pounce_cooldown: 0.0,
            running: false,
            pouncing: false,
            pounce_hits: false,
            holding_for: 0.0,
            caught_for: 0.0,
            catch_cooldown: 0.0,
            hack_left: 0.0,
            hairball: None,
            pig: false,
            gobble_for: 0.0,
            stretch: Stretch::Bow,
            stretch_for: 0.0,
            activity: Activity::Walking,
            time_left: 6.0,
            clock: 0.0,
        }
    }

    pub fn activity(&self) -> Activity {
        self.activity
    }

    pub fn is_held(&self) -> bool {
        self.activity == Activity::Held
    }

    /// Picks the cat up; it follows `drag_to` until `release`.
    pub fn grab(&mut self) {
        self.activity = Activity::Held;
        self.drag_seen = (self.x, self.y);
        self.vx = 0.0;
        self.vy = 0.0;
        self.swing = 0.0;
        self.swing_v = 0.0;
        self.held_for = 0.0;
    }

    pub fn drag_to(&mut self, x: f64, y: f64) {
        if self.is_held() {
            self.x = x;
            self.y = y;
        }
    }

    /// Measures heights from a ground `rise` points higher from now on (the
    /// cat is on another screen); the cat itself stays where it is.
    pub fn raise_ground(&mut self, rise: f64) {
        self.y -= rise;
        self.drag_seen.1 -= rise;
    }

    /// Lets go: the cat keeps the drag's momentum and falls.
    pub fn release(&mut self) {
        if !self.is_held() {
            return;
        }
        self.activity = Activity::Falling;
        self.vx = self.vx.clamp(-MAX_THROW_SPEED, MAX_THROW_SPEED);
        self.vy = self.vy.clamp(-MAX_THROW_SPEED, MAX_THROW_SPEED);
        if self.vx.abs() > FACE_THROW_SPEED {
            self.facing = if self.vx > 0.0 {
                Facing::Right
            } else {
                Facing::Left
            };
        }
    }

    pub fn is_petted(&self) -> bool {
        self.petted_left > 0.0
    }

    /// The mouse moved `distance` points over the cat. Enough of it, on a cat
    /// that has its feet on the ground, makes it happy for a moment.
    pub fn stroke(&mut self, distance: f64) {
        // A shaken pointer is prey, not a hand.
        let hunting = self.activity == Activity::Hunting || self.shake.is_shaking();
        if !self.is_grounded() || hunting {
            return;
        }
        self.affection = (self.affection + distance.abs()).min(PET_THRESHOLD);
        if self.affection >= PET_THRESHOLD {
            self.petted_left = PET_LINGER;
        }
    }

    /// Advances the cat by `dt` seconds within `bounds`.
    pub fn update(&mut self, dt: f64, bounds: Bounds, inputs: Inputs, rng: &mut Rng) {
        self.clock += dt;
        self.affection = (self.affection - AFFECTION_DECAY * dt).max(0.0);
        self.petted_left = (self.petted_left - dt).max(0.0);
        self.run_prey(dt, bounds);
        self.age_hairball(dt);
        self.catch_cooldown = (self.catch_cooldown - dt).max(0.0);
        if self.activity == Activity::Held {
            // Picked up: the pointer goes free.
            self.holding_for = 0.0;
        }
        match self.activity {
            Activity::Held => return self.follow_drag(dt, bounds),
            Activity::Falling => {
                self.fall(dt, bounds, rng);
                return self.snatch_in_flight(rng);
            }
            Activity::Clinging => return self.cling(dt),
            Activity::Walking
            | Activity::Idle
            | Activity::Sleeping
            | Activity::Eating
            | Activity::Chasing
            | Activity::Hunting
            | Activity::Hacking
            | Activity::Gobbling
            | Activity::Stretching
            | Activity::Scratching => {}
        }
        self.x = self.x.clamp(bounds.min_x, bounds.max_x);
        self.y = 0.0;

        // Before petting: a shake over the cat starts a hunt, not a cuddle.
        if !inputs.holds_cat() {
            self.notice_shake();
        }
        if inputs.holds_cat() || self.is_petted() {
            self.activity = Activity::Idle;
            self.running = false;
            self.holding_for = 0.0;
            self.time_left = rng.range(1.0, 3.0);
            return;
        }

        let hunting = self.hunt_left > 0.0 || self.holding_for > 0.0;
        if hunting && self.activity == Activity::Idle {
            // Back on its feet after a pounce, maybe with the pointer.
            self.activity = Activity::Hunting;
        }
        match self.activity {
            Activity::Eating => return self.eat(dt),
            Activity::Chasing => return self.chase(dt, bounds, rng),
            Activity::Hunting => return self.hunt(dt, bounds, rng),
            Activity::Hacking => return self.hack(dt, bounds),
            Activity::Gobbling => return self.gobble(dt),
            Activity::Stretching => return self.stretch(dt, rng),
            Activity::Scratching => return self.scratch(dt, rng),
            Activity::Walking => {
                if let Some(wall) = self.walk(dt, bounds.min_x, bounds.max_x)
                    && self.maybe_scratch(wall, bounds, rng)
                {
                    return;
                }
            }
            _ => {}
        }

        self.time_left -= dt;
        if self.time_left <= 0.0 {
            self.pick_next_activity(bounds, rng);
        }
    }

    fn follow_drag(&mut self, dt: f64, bounds: Bounds) {
        self.x = bounds.between_walls(self.x);
        self.y = bounds.under_ceiling(self.y);
        if dt > 0.0 {
            let (seen_x, seen_y) = self.drag_seen;
            let lerp = |v: f64, target: f64| v + (target - v) * DRAG_SMOOTHING;
            self.vx = lerp(self.vx, (self.x - seen_x) / dt);
            self.vy = lerp(self.vy, (self.y - seen_y) / dt);
        }
        self.drag_seen = (self.x, self.y);
        self.held_for += dt;
        self.swing_pendulum(dt);
    }

    /// The body trails behind the drag, overshoots when it stops, then settles.
    fn swing_pendulum(&mut self, dt: f64) {
        let rest = (-self.vx / LEAN_SPEED).clamp(-MAX_SWING, MAX_SWING);
        let accel = -SWING_STIFFNESS * (self.swing - rest) - SWING_DAMPING * self.swing_v;
        self.swing_v += accel * dt;
        self.swing = (self.swing + self.swing_v * dt).clamp(-MAX_SWING, MAX_SWING);
    }

    fn dangle_pose(&self) -> Pose {
        // Sprites are drawn facing right and mirrored, so the swing must be too.
        let swing = match self.facing {
            Facing::Right => self.swing,
            Facing::Left => -self.swing,
        };
        let kick = if self.held_for >= KICK_SECONDS {
            Kick::None
        } else if ((self.held_for * KICK_FPS) as u64).is_multiple_of(2) {
            Kick::Left
        } else {
            Kick::Right
        };
        Pose::Dangle {
            swing: swing.round() as i8,
            kick,
        }
    }

    fn fall(&mut self, dt: f64, bounds: Bounds, rng: &mut Rng) {
        self.vy -= GRAVITY * dt;
        self.x += self.vx * dt;
        self.y += self.vy * dt;

        let at_wall = (self.x <= bounds.min_x && !bounds.open.left)
            || (self.x >= bounds.max_x && !bounds.open.right);
        if at_wall {
            self.x = self.x.clamp(bounds.min_x, bounds.max_x);
            if self.vx.abs() >= CLING_MIN_SPEED && self.y >= CLING_MIN_HEIGHT {
                return self.grab_wall(rng);
            }
            self.vx = -self.vx * WALL_BOUNCE;
        }
        if self.y >= bounds.max_y && !bounds.open.top {
            self.y = bounds.max_y;
            if self.vy >= CLING_MIN_SPEED {
                return self.grab_ceiling(rng);
            }
            self.vy = self.vy.min(0.0);
        }
        // Thrown up from the screen below, it rises past the ground before it can land.
        if self.y <= 0.0 && self.vy <= 0.0 {
            // Cats land on their feet, then take a moment.
            self.y = 0.0;
            self.vx = 0.0;
            self.vy = 0.0;
            self.activity = Activity::Idle;
            self.time_left = rng.range(1.5, 3.0);
        }
    }

    /// Walks on, turning back at the screen's edges; returns the side of
    /// the edge it walked into, if any.
    fn walk(&mut self, dt: f64, min_x: f64, max_x: f64) -> Option<Facing> {
        let step = WALK_SPEED * dt;
        match self.facing {
            Facing::Right => self.x += step,
            Facing::Left => self.x -= step,
        }
        if self.x >= max_x {
            self.x = max_x;
            self.facing = Facing::Left;
            Some(Facing::Right)
        } else if self.x <= min_x {
            self.x = min_x;
            self.facing = Facing::Right;
            Some(Facing::Left)
        } else {
            None
        }
    }

    fn pick_next_activity(&mut self, bounds: Bounds, rng: &mut Rng) {
        let roll = rng.next_f64();
        if self.activity == Activity::Idle {
            let started = if roll >= 1.0 - SNACK_CHANCE {
                self.feed(bounds)
            } else if roll >= 1.0 - SNACK_CHANCE - MOUSE_CHANCE {
                self.release_mouse(bounds, rng)
            } else if roll >= 1.0 - SNACK_CHANCE - MOUSE_CHANCE - HAIRBALL_CHANCE {
                self.cough_up()
            } else if roll >= 1.0 - SNACK_CHANCE - MOUSE_CHANCE - HAIRBALL_CHANCE - STRETCH_CHANCE {
                self.stretch_out(rng)
            } else {
                false
            };
            if started {
                return;
            }
        }
        // Waking from a nap, a cat stretches before anything else.
        if self.activity == Activity::Sleeping
            && roll < WAKE_STRETCH_CHANCE
            && self.stretch_out(rng)
        {
            return;
        }
        let (activity, min, max) = match self.activity {
            Activity::Walking if roll < 0.2 => (Activity::Sleeping, 12.0, 30.0),
            Activity::Walking => (Activity::Idle, 2.0, 6.0),
            Activity::Idle if roll < 0.1 => (Activity::Sleeping, 12.0, 30.0),
            Activity::Idle
            | Activity::Sleeping
            | Activity::Held
            | Activity::Falling
            | Activity::Clinging
            | Activity::Eating
            | Activity::Chasing
            | Activity::Hunting
            | Activity::Hacking
            | Activity::Gobbling
            | Activity::Stretching
            | Activity::Scratching => (Activity::Walking, 4.0, 10.0),
        };
        if activity == Activity::Walking {
            self.facing = if rng.next_f64() < 0.5 {
                Facing::Left
            } else {
                Facing::Right
            };
        }
        self.activity = activity;
        self.time_left = rng.range(min, max);
    }

    /// Paws on the screen edge it hit, facing the wall, for a moment.
    fn grab_wall(&mut self, rng: &mut Rng) {
        self.facing = if self.vx > 0.0 {
            Facing::Right
        } else {
            Facing::Left
        };
        self.vx = 0.0;
        self.vy = 0.0;
        self.start_clinging(ClingSide::Wall, rng);
        self.cling_slides = rng.next_f64() < SLIDE_CHANCE;
    }

    /// Paws on the top of the screen, upside down, for a moment.
    fn grab_ceiling(&mut self, rng: &mut Rng) {
        self.vx = 0.0;
        self.vy = 0.0;
        self.start_clinging(ClingSide::Ceiling, rng);
    }

    fn start_clinging(&mut self, side: ClingSide, rng: &mut Rng) {
        self.activity = Activity::Clinging;
        self.cling_side = side;
        self.cling_slides = false;
        self.slide_speed = 0.0;
        self.clinging_for = 0.0;
        self.time_left = rng.range(1.2, 2.0);
    }

    /// Hangs on, then lets go: off the ceiling it just drops, off a wall it
    /// pushes away with a small hop.
    fn cling(&mut self, dt: f64) {
        self.clinging_for += dt;
        if self.slide_speed > 0.0 {
            return self.slide_down(dt);
        }
        self.time_left -= dt;
        if self.time_left > 0.0 {
            return;
        }
        if self.cling_side == ClingSide::Wall && self.cling_slides {
            self.slide_speed = SLIDE_START_SPEED;
            return;
        }
        self.activity = Activity::Falling;
        if self.cling_side == ClingSide::Ceiling {
            return;
        }
        let away = match self.facing {
            Facing::Right => -1.0,
            Facing::Left => 1.0,
        };
        self.vx = away * JUMP_OFF_SPEED;
        self.vy = JUMP_OFF_LIFT;
        self.facing = if away > 0.0 {
            Facing::Right
        } else {
            Facing::Left
        };
    }

    /// Slips down the wall, faster and faster, until its feet reach the
    /// ground; then it turns away from the wall.
    fn slide_down(&mut self, dt: f64) {
        self.slide_speed = (self.slide_speed + SLIDE_ACCELERATION * dt).min(SLIDE_MAX_SPEED);
        self.y -= self.slide_speed * dt;
        if self.y > 0.0 {
            return;
        }
        self.y = 0.0;
        self.slide_speed = 0.0;
        self.facing = match self.facing {
            Facing::Right => Facing::Left,
            Facing::Left => Facing::Right,
        };
        self.activity = Activity::Idle;
        self.time_left = 1.5;
    }

    fn cling_pose(&self) -> Pose {
        // Paws scrabble when it grabs the wall, and all the way down a slide.
        let scrabbling = self.clinging_for < SCRABBLE_SECONDS || self.slide_speed > 0.0;
        let phase = if scrabbling {
            (self.clinging_for * WALK_FPS * 2.0) as u64 as u8 % 4
        } else {
            1
        };
        match self.cling_side {
            ClingSide::Wall => Pose::Cling(phase),
            ClingSide::Ceiling => Pose::Ceiling(phase),
        }
    }

    pub fn look(&self, inputs: Inputs) -> Look {
        if self.is_grounded() && self.is_petted() {
            // Eyes shut with delight, hearts floating up.
            return Look {
                pose: Pose::Blink,
                bubble: Bubble::Hearts((self.clock * HEARTS_FPS) as u64 as u8),
                facing: self.facing,
            };
        }
        let pose = match self.activity {
            Activity::Eating | Activity::Chasing | Activity::Hunting => {
                self.play_pose().unwrap_or(Pose::Stand)
            }
            Activity::Hacking => self.hack_pose(),
            Activity::Gobbling => self.gobble_pose(),
            Activity::Stretching => self.stretch_pose(),
            Activity::Scratching => self.scratch_pose(),
            Activity::Walking => Pose::Walk((self.clock * WALK_FPS) as u64 as u8 % 4),
            Activity::Sleeping => Pose::Sleep,
            Activity::Held => self.dangle_pose(),
            Activity::Falling => Pose::Fall,
            Activity::Clinging => self.cling_pose(),
            Activity::Idle if self.clock % BLINK_PERIOD > BLINK_PERIOD - BLINK_LENGTH => {
                Pose::Blink
            }
            Activity::Idle => Pose::Stand,
        };
        let reared = matches!(
            self.activity,
            Activity::Held | Activity::Clinging | Activity::Scratching
        );
        let bubble = if reared {
            // The cat's body fills the bubble's spot above its head.
            Bubble::None
        } else if inputs.busy {
            Bubble::Dots((self.clock * DOTS_FPS) as u64 as u8 % 3 + 1)
        } else if inputs.unread || inputs.news || self.is_startled() {
            Bubble::Bang
        } else if self.activity == Activity::Sleeping {
            Bubble::Zzz((self.clock * ZZZ_FPS) as u64 as u8)
        } else {
            self.gobble_bubble().unwrap_or(Bubble::None)
        };
        Look {
            pose,
            bubble,
            facing: self.facing,
        }
    }
}

#[cfg(test)]
mod hairball_tests;
#[cfg(test)]
mod play_tests;
#[cfg(test)]
mod stretch_tests;
#[cfg(test)]
mod tests;
