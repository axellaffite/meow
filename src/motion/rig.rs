//! Helpers for procedural layers, which move a bone in creature axes
//! whatever its own axes are, and for turning a pose into world matrices
//! without allocating.
//!
//! An export often gives bones rotated rest frames (a bone along an arm
//! points sideways): a layer says what it wants in creature space (glTF: +X
//! the creature's left, +Y up, +Z toward the viewer) and these helpers turn
//! it into the bone's local transform.

use glam::{Mat4, Quat, Vec3};

use crate::creature::asset::Skeleton;
use crate::creature::pose::{Pose, Transform};

/// A scale along the creature's axes as the scale along a bone's own axes,
/// for a bone whose rest world rotation is `w` (exact when its axes line up
/// with the creature's).
pub fn local_scale(w: Quat, s: Vec3) -> Vec3 {
    let axis = |a: Vec3| {
        let a = w * a;
        let a2 = a * a;
        a2.x * s.x + a2.y * s.y + a2.z * s.z
    };
    Vec3::new(axis(Vec3::X), axis(Vec3::Y), axis(Vec3::Z))
}

/// Whether `bone` is `ancestor` or under it.
pub fn descends(skeleton: &Skeleton, bone: usize, ancestor: usize) -> bool {
    let mut at = Some(bone);
    let mut guard = 0;
    while let Some(index) = at {
        if index == ancestor {
            return true;
        }
        guard += 1;
        if guard > skeleton.joints.len() {
            return false;
        }
        at = skeleton.joints.get(index).and_then(|j| j.parent);
    }
    false
}

/// The rotation that takes the direction `from` to `to`.
pub fn arc(from: Vec3, to: Vec3) -> Quat {
    let (a, b) = (from.normalize_or_zero(), to.normalize_or_zero());
    if a == Vec3::ZERO || b == Vec3::ZERO {
        return Quat::IDENTITY;
    }
    Quat::from_rotation_arc(a, b)
}

/// The world matrices of `pose`, written into `out` (no allocation once sized).
pub fn world_into(pose: &Pose, skeleton: &Skeleton, out: &mut Vec<Mat4>) {
    out.resize(skeleton.joints.len(), Mat4::IDENTITY);
    for &index in skeleton.order() {
        let joint = &skeleton.joints[index];
        let local = pose.locals.get(index).unwrap_or(&joint.rest).matrix();
        let parent = joint.parent.map_or(skeleton.root, |parent| out[parent]);
        out[index] = parent * local;
    }
}

/// A transform with every part finite, or `fallback`.
pub fn sane(t: Transform, fallback: Transform) -> Transform {
    if t.t.is_finite() && t.r.is_finite() && t.s.is_finite() && t.r.length_squared() > 1e-8 {
        t
    } else {
        fallback
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_creature_axis_scale_lands_on_the_matching_local_axis() {
        // A bone whose local Y points +Z and local Z points +Y.
        let w = Quat::from_xyzw(0.0, -0.70710677, -0.70710677, 0.0);
        let local = local_scale(w, Vec3::new(1.0, 0.8, 1.2));
        assert!(
            (local - Vec3::new(1.0, 1.2, 0.8)).length() < 1e-5,
            "{local}"
        );
        assert_eq!(
            local_scale(Quat::IDENTITY, Vec3::new(2.0, 3.0, 4.0)),
            Vec3::new(2.0, 3.0, 4.0)
        );
    }

    #[test]
    fn arcs_turn_one_direction_onto_another() {
        let q = arc(Vec3::Y, Vec3::X);
        assert!((q * Vec3::Y - Vec3::X).length() < 1e-5);
        assert_eq!(arc(Vec3::ZERO, Vec3::X), Quat::IDENTITY);
    }
}
