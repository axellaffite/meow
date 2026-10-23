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
}
