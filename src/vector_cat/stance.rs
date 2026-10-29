//! How the standing animal holds itself in each upright pose: legs, head,
//! back, rump and tail, shared by the cat and the pig.

use crate::sprites::SPIT;

use super::paint::P;

const WALK_STRIDES: [[f64; 4]; 4] = [
    [1.0, -1.0, -1.0, 1.0],
    [0.0; 4],
    [-1.0, 1.0, 1.0, -1.0],
    [0.0; 4],
];
/// Legs off the ground mid-stride, in the phases between two strides.
const WALK_LIFTS: [[f64; 4]; 4] = [
    [0.0; 4],
    [0.0, 0.7, 0.7, 0.0],
    [0.0; 4],
    [0.7, 0.0, 0.0, 0.7],
];
/// Head drop and forward reach for each `Pose::Hack` phase, as in the sprite.
const HACK_HEADS: [(f64, f64); 3] = [(1.0, 0.0), (2.0, 1.0), (3.0, 1.0)];
/// Head drop between bites, as in the sprite.
const BITE_DROPS: [f64; 2] = [3.0, 4.0];
/// The column a bow tips the body about: behind it rises, ahead sinks.
pub(super) const BOW_PIVOT: f64 = 11.0;
/// How steeply the body tips in each `Pose::Bow` phase, and how far the
/// front paws reach ahead.
const BOWS: [(f64, f64); 2] = [(0.1, 1.2), (0.2, 3.2)];
/// How high the back rises in each `Pose::Arch` phase.
const ARCHES: [f64; 2] = [1.5, 4.0];
/// The front paws' shift for each `Pose::Scratch` phase, raking the wall in
/// turn.
const SCRATCH_STRIDES: [(f64, f64); 4] = [(-1.2, 1.2), (0.0, 0.0), (1.2, -1.2), (0.0, 0.0)];

/// One leg: its foot shifted by `dx` and lifted by `lift`, or its paw raised
/// to `paw`.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Leg {
    pub dx: f64,
    pub lift: f64,
    pub paw: Option<P>,
}

/// How the standing animal holds itself.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Stance {
    pub closed: bool,
    pub legs: [Leg; 4],
    /// The head moved this far from its place.
    pub head: P,
    /// The head turned nose down, in radians.
    pub tilt: f64,
    /// The middle of the back raised.
    pub hunch: f64,
    /// The rump lowered toward the floor.
    pub rump: f64,
    pub tail_up: bool,
    pub mouth_open: bool,
    /// The whole animal tipped this steeply about `BOW_PIVOT` (front down),
    /// its paws kept on the ground.
    pub slope: f64,
}

impl Stance {
    pub fn walking(phase: u8) -> Self {
        let phase = usize::from(phase) % WALK_STRIDES.len();
        let mut legs = [Leg::default(); 4];
        for (i, leg) in legs.iter_mut().enumerate() {
            leg.dx = WALK_STRIDES[phase][i] * 0.9;
            leg.lift = WALK_LIFTS[phase][i];
        }
        Self {
            legs,
            ..Self::default()
        }
    }

    pub fn blinking() -> Self {
        Self {
            closed: true,
            ..Self::default()
        }
    }

    /// Mid-air: legs spread fore and aft, off the ground.
    pub fn falling() -> Self {
        let spread = [(-1.5, 0.9), (-1.0, 0.5), (1.0, 0.5), (1.5, 0.9)];
        Self {
            legs: spread.map(|(dx, lift)| Leg {
                dx,
                lift,
                paw: None,
            }),
            tail_up: true,
            ..Self::default()
        }
    }

    /// Head down in the bowl in front, eyes shut.
    pub fn eating(bite: u8) -> Self {
        let drop = BITE_DROPS[usize::from(bite) % BITE_DROPS.len()];
        Self {
            closed: true,
            head: (1.0, drop),
            tilt: 0.3,
            ..Self::default()
        }
    }

    /// The near front paw raised in front of the chin, or slapped down ahead
    /// (over `sprites::SWAT_PAW`).
    pub fn swatting(phase: u8) -> Self {
        let mut legs = [Leg::default(); 4];
        legs[3].paw = Some(if phase.is_multiple_of(2) {
            (21.4, 13.6)
        } else {
            (22.4, 21.2)
        });
        Self {
            legs,
            ..Self::default()
        }
    }

    /// Hunched over, head down and forward, coughing.
    pub fn hacking(phase: u8) -> Self {
        let phase = phase.min(SPIT);
        let (drop, reach) = HACK_HEADS[usize::from(phase)];
        Self {
            closed: true,
            head: (reach, drop),
            tilt: 0.12 * f64::from(phase),
            hunch: if phase > 0 { 1.3 } else { 0.0 },
            mouth_open: phase == SPIT,
            ..Self::default()
        }
    }

    /// Rump sunk toward the floor, tail up, eyes shut.
    pub fn squatting(phase: u8) -> Self {
        Self {
            closed: true,
            rump: if phase == 0 { 1.0 } else { 2.0 },
            tail_up: true,
            ..Self::default()
        }
    }

    /// Rump up, chest down, front paws reaching ahead, tail up.
    pub fn bowing(phase: u8) -> Self {
        let full = phase > 0;
        let (slope, reach) = BOWS[usize::from(full)];
        let mut legs = [Leg::default(); 4];
        for leg in &mut legs[2..] {
            leg.dx = reach;
        }
        Self {
            closed: full,
            legs,
            head: (0.8, 0.0),
            tilt: 0.15,
            tail_up: true,
            slope,
            ..Self::default()
        }
    }

    /// The back arched high, paws drawn in under it, head low.
    pub fn arching(phase: u8) -> Self {
        let full = phase > 0;
        let mut legs = [Leg::default(); 4];
        for (i, leg) in legs.iter_mut().enumerate() {
            leg.dx = if i < 2 { 0.6 } else { -0.6 };
        }
        Self {
            closed: full,
            legs,
            head: (0.0, if full { 1.2 } else { 0.4 }),
            tilt: 0.15,
            hunch: ARCHES[usize::from(full)],
            ..Self::default()
        }
    }

    /// Standing, the front paws raking forward and back in turn; drawn
    /// turned up against the wall like `Pose::Cling`.
    pub fn scratching(phase: u8) -> Self {
        let (far, near) = SCRATCH_STRIDES[usize::from(phase) % SCRATCH_STRIDES.len()];
        let mut legs = [Leg::default(); 4];
        legs[2].dx = far;
        legs[3].dx = near;
        Self {
            legs,
            ..Self::default()
        }
    }
}
