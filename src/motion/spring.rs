//! Damped springs solved exactly, so they hold at any step.
//!
//! `x'' = -w^2 (x - target) - 2 z w x' + a` has a closed form over a step
//! when the target and the outside acceleration `a` hold still: a step of
//! any length lands where the true motion would, and never blows up. The
//! callers cut a long frame into a few steps and slide the target along
//! them, so a target that moves is followed the same at 120 or 12 frames
//! per second.

use std::f32::consts::TAU;
use std::ops::{Add, Mul, Sub};

/// What a spring can carry: `f32`, `Vec2`, `Vec3`.
pub trait SpringValue:
    Copy + Add<Output = Self> + Sub<Output = Self> + Mul<f32, Output = Self>
{
    fn zero() -> Self;
    fn finite(self) -> bool;
}

impl SpringValue for f32 {
    fn zero() -> Self {
        0.0
    }
    fn finite(self) -> bool {
        self.is_finite()
    }
}

impl SpringValue for glam::Vec2 {
    fn zero() -> Self {
        Self::ZERO
    }
    fn finite(self) -> bool {
        self.is_finite()
    }
}

impl SpringValue for glam::Vec3 {
    fn zero() -> Self {
        Self::ZERO
    }
    fn finite(self) -> bool {
        self.is_finite()
    }
}

/// The step's response: after `h` seconds, `y = a y0 + b v0` and
/// `v = c y0 + d v0`, `y` being the offset from the target.
fn response(omega: f32, zeta: f32, h: f32) -> [f32; 4] {
    if omega <= 0.0 || h <= 0.0 {
        return [1.0, h.max(0.0), 0.0, 1.0];
    }
    let zeta = zeta.max(0.0);
    if (zeta - 1.0).abs() < 1e-3 {
        let e = (-omega * h).exp();
        return [
            (1.0 + omega * h) * e,
            h * e,
            -omega * omega * h * e,
            (1.0 - omega * h) * e,
        ];
    }
    if zeta < 1.0 {
        let wd = omega * (1.0 - zeta * zeta).sqrt();
        let e = (-zeta * omega * h).exp();
        let (s, c) = (wd * h).sin_cos();
        let k = zeta * omega / wd;
        return [
            e * (c + k * s),
            e * s / wd,
            -e * omega * omega * s / wd,
            e * (c - k * s),
        ];
    }
    let root = (zeta * zeta - 1.0).sqrt();
    let (r1, r2) = (-omega * (zeta - root), -omega * (zeta + root));
    let (e1, e2) = ((r1 * h).exp(), (r2 * h).exp());
    let span = r2 - r1;
    // y = A e1 + B e2 with B = (v0 - r1 y0) / span, A = y0 - B.
    [
        (r2 * e1 - r1 * e2) / span,
        (e2 - e1) / span,
        r1 * r2 * (e1 - e2) / span,
        (r2 * e2 - r1 * e1) / span,
    ]
}

/// A value pulled toward a target.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spring<T: SpringValue> {
    pub value: T,
    pub velocity: T,
    /// Natural frequency, hertz.
    pub hz: f32,
    /// Damping ratio: under 1 it rings, 1 it settles fastest without overshoot.
    pub zeta: f32,
}

impl<T: SpringValue> Spring<T> {
    pub fn new(value: T, hz: f32, zeta: f32) -> Self {
        Self {
            value,
            velocity: T::zero(),
            hz,
            zeta,
        }
    }

    /// One exact step of `h` seconds toward `target`, with an outside
    /// acceleration `accel` (gravity, inertia) and the damping times `damp`.
    pub fn step(&mut self, target: T, accel: T, damp: f32, h: f32) {
        let omega = self.hz * TAU;
        // A constant acceleration only moves the rest point.
        let rest = if omega > 0.0 {
            target + accel * (1.0 / (omega * omega))
        } else {
            target
        };
        let [a, b, c, d] = response(omega, self.zeta * damp, h);
        let y = self.value - rest;
        let v = self.velocity;
        self.value = rest + y * a + v * b;
        self.velocity = y * c + v * d;
        if !self.value.finite() || !self.velocity.finite() {
            self.value = target;
            self.velocity = T::zero();
        }
    }

    /// Advances `dt` in steps of at most `max_step`, the target sliding from
    /// `from` to `to` over the frame.
    pub fn follow(&mut self, from: T, to: T, accel: T, damp: f32, dt: f32, max_step: f32) {
        let steps = substeps(dt, max_step);
        let h = dt / steps as f32;
        for i in 1..=steps {
            let w = i as f32 / steps as f32;
            self.step(from + (to - from) * w, accel, damp, h);
        }
    }

    /// Adds a kick to the velocity.
    pub fn kick(&mut self, impulse: T) {
        self.velocity = self.velocity + impulse;
    }

    pub fn reset(&mut self, value: T) {
        self.value = value;
        self.velocity = T::zero();
    }
}

/// How many steps cut `dt` into pieces of at most `max_step` (1 to 64).
pub fn substeps(dt: f32, max_step: f32) -> usize {
    if dt.is_nan() || max_step.is_nan() || dt <= 0.0 || max_step <= 0.0 {
        return 1;
    }
    // A frame that is a whole number of steps (1/120 s in 1/240 s steps)
    // takes exactly that many, rounding error aside.
    ((dt / max_step - 1e-3).ceil() as usize).clamp(1, 64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;

    fn settle(hz: f32, zeta: f32, dt: f32, seconds: f32) -> Spring<f32> {
        let mut spring = Spring::new(1.0, hz, zeta);
        let steps = (seconds / dt).round() as usize;
        for _ in 0..steps {
            spring.step(0.0, 0.0, 1.0, dt);
        }
        spring
    }

    #[test]
    fn every_damping_settles_on_the_target() {
        for zeta in [0.1, 0.35, 0.999, 1.0, 1.001, 2.5] {
            let spring = settle(3.0, zeta, 1.0 / 60.0, 8.0);
            assert!(spring.value.abs() < 1e-3, "zeta {zeta}: {}", spring.value);
        }
    }

    #[test]
    fn the_step_length_does_not_change_the_motion() {
        // The closed form is exact: 120 small steps and 12 big ones agree.
        let fine = settle(3.2, 0.35, 1.0 / 120.0, 0.5);
        let coarse = settle(3.2, 0.35, 1.0 / 12.0, 0.5);
        assert!((fine.value - coarse.value).abs() < 1e-4);
        assert!((fine.velocity - coarse.velocity).abs() < 1e-3);
    }

    #[test]
    fn huge_steps_and_stiff_springs_stay_finite() {
        for dt in [1e-6, 1.0 / 120.0, 1.0 / 12.0, 1.0, 30.0] {
            for hz in [0.0, 0.5, 6.0, 400.0] {
                let mut spring = Spring::new(Vec3::splat(5.0), hz, 0.3);
                spring.kick(Vec3::splat(1e4));
                for _ in 0..50 {
                    spring.step(Vec3::ZERO, Vec3::new(0.0, -9.8, 0.0), 3.0, dt);
                    assert!(spring.value.is_finite() && spring.velocity.is_finite());
                }
                if hz >= 6.0 && dt >= 1.0 / 12.0 {
                    let rest = Vec3::new(0.0, -9.8, 0.0) / (hz * TAU).powi(2);
                    assert!((spring.value - rest).length() < 1e-2, "{hz} {dt}");
                }
            }
        }
    }

    #[test]
    fn gravity_moves_the_rest_point() {
        let mut spring = Spring::new(0.0, 2.0, 1.0);
        for _ in 0..600 {
            spring.step(0.0, -9.8, 1.0, 1.0 / 60.0);
        }
        let omega = 2.0 * TAU;
        assert!((spring.value + 9.8 / (omega * omega)).abs() < 1e-4);
    }

    #[test]
    fn an_underdamped_spring_overshoots_once_kicked() {
        let mut spring = Spring::new(0.0, 3.0, 0.3);
        spring.kick(1.0);
        let mut low = 0.0f32;
        for _ in 0..120 {
            spring.step(0.0, 0.0, 1.0, 1.0 / 60.0);
            low = low.min(spring.value);
        }
        assert!(low < -0.01, "it swings back past rest");
    }

    #[test]
    fn substeps_are_bounded() {
        assert_eq!(substeps(1.0 / 120.0, 1.0 / 240.0), 2);
        assert_eq!(substeps(1.0 / 12.0, 1.0 / 240.0), 20);
        assert_eq!(substeps(100.0, 1.0 / 240.0), 64);
        assert_eq!(substeps(f32::NAN, 0.01), 1);
        assert_eq!(substeps(-1.0, 0.01), 1);
    }
}
