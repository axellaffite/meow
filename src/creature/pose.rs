//! Poses: one local transform per joint, and what they give in the world.
//!
//! A [`Pose`] holds the joints' local transforms (relative to their parent
//! joint) in the skeleton's order; [`Pose::world`] chains them into world
//! matrices (glTF space, meters), and [`Pose::skin`] turns those into the
//! matrices the vertex shader skins with.

use glam::{Mat4, Quat, Vec3};

use super::asset::{Skeleton, Socket};

/// A translation, rotation and scale, applied scale first.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    pub t: Vec3,
    pub r: Quat,
    pub s: Vec3,
}

impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Transform {
    pub const IDENTITY: Self = Self {
        t: Vec3::ZERO,
        r: Quat::IDENTITY,
        s: Vec3::ONE,
    };

    pub fn new(t: Vec3, r: Quat, s: Vec3) -> Self {
        Self { t, r, s }
    }

    pub fn matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.s, self.r, self.t)
    }

    /// The transform of an affine matrix without shear.
    pub fn from_matrix(matrix: Mat4) -> Self {
        let (s, r, t) = matrix.to_scale_rotation_translation();
        Self { t, r, s }
    }

    /// `self` moved `w` of the way to `other` (0 keeps `self`, 1 gives
    /// `other`): translation and scale lerped, rotation slerped along the
    /// shortest arc.
    pub fn blend(&self, other: &Transform, w: f32) -> Transform {
        Transform {
            t: self.t.lerp(other.t, w),
            r: self.r.slerp(other.r, w).normalize(),
            s: self.s.lerp(other.s, w),
        }
    }
}

/// One local transform per joint of a [`Skeleton`], in its order.
#[derive(Clone, Debug, PartialEq)]
pub struct Pose {
    pub locals: Vec<Transform>,
}

impl Pose {
    /// Every joint at its rest (bind) transform.
    pub fn rest(skeleton: &Skeleton) -> Self {
        Self {
            locals: skeleton.joints.iter().map(|joint| joint.rest).collect(),
        }
    }

    /// `self` moved `w` of the way to `other`, joint by joint. Joints past the
    /// shorter of the two keep `self`'s transform.
    pub fn blend(&self, other: &Pose, w: f32) -> Pose {
        let locals = self
            .locals
            .iter()
            .enumerate()
            .map(|(i, local)| match other.locals.get(i) {
                Some(theirs) => local.blend(theirs, w),
                None => *local,
            })
            .collect();
        Pose { locals }
    }

    /// Each joint's world matrix (the skeleton's root placement included), in
    /// the skeleton's order. A joint the pose has no transform for stays at rest.
    pub fn world(&self, skeleton: &Skeleton) -> Vec<Mat4> {
        let mut world = vec![Mat4::IDENTITY; skeleton.joints.len()];
        for &index in skeleton.order() {
            let joint = &skeleton.joints[index];
            let local = self.locals.get(index).unwrap_or(&joint.rest).matrix();
            let parent = joint.parent.map_or(skeleton.root, |parent| world[parent]);
            world[index] = parent * local;
        }
        world
    }

    /// The skinning matrices, world times inverse bind, of the skin joints
    /// (the first `skeleton.skin_joints`, which `JOINTS_0` indexes).
    pub fn skin(&self, skeleton: &Skeleton) -> Vec<Mat4> {
        skin_matrices(&self.world(skeleton), skeleton)
    }
}

/// [`Pose::skin`] from world matrices already computed.
pub fn skin_matrices(world: &[Mat4], skeleton: &Skeleton) -> Vec<Mat4> {
    skeleton.joints[..skeleton.skin_joints]
        .iter()
        .zip(world)
        .map(|(joint, world)| *world * joint.inverse_bind)
        .collect()
}

/// Where `socket` is in the world, as a matrix, given the joints' world
/// matrices (from [`Pose::world`]).
pub fn socket_matrix(world: &[Mat4], socket: &Socket) -> Mat4 {
    world.get(socket.joint).copied().unwrap_or(Mat4::IDENTITY) * socket.local.matrix()
}

/// Where `socket` is in the world.
pub fn socket_position(world: &[Mat4], socket: &Socket) -> Vec3 {
    socket_matrix(world, socket).transform_point3(Vec3::ZERO)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature::asset::Joint;

    fn joint(name: &str, parent: Option<usize>, t: Vec3) -> Joint {
        let rest = Transform::new(t, Quat::IDENTITY, Vec3::ONE);
        Joint {
            name: name.into(),
            parent,
            rest,
            inverse_bind: Mat4::IDENTITY,
            extras: serde_json::Value::Null,
        }
    }

    /// root at the origin, body 0.3 up, head 0.1 above the body; the head is
    /// listed before its parent, as a non-topological order can be.
    fn skeleton() -> Skeleton {
        let mut joints = vec![
            joint("root", None, Vec3::ZERO),
            joint("head", Some(2), Vec3::new(0.0, 0.1, 0.0)),
            joint("body", Some(0), Vec3::new(0.0, 0.3, 0.0)),
        ];
        let rest = Pose {
            locals: joints.iter().map(|j| j.rest).collect(),
        };
        let skeleton = Skeleton::new(joints.clone(), 3, Mat4::IDENTITY).unwrap();
        let world = rest.world(&skeleton);
        for (joint, world) in joints.iter_mut().zip(&world) {
            joint.inverse_bind = world.inverse();
        }
        Skeleton::new(joints, 3, Mat4::IDENTITY).unwrap()
    }

    #[test]
    fn world_matrices_chain_parents_whatever_the_order() {
        let skeleton = skeleton();
        let world = Pose::rest(&skeleton).world(&skeleton);
        let head = world[1].transform_point3(Vec3::ZERO);
        assert!((head - Vec3::new(0.0, 0.4, 0.0)).length() < 1e-6, "{head}");
    }

    #[test]
    fn rest_pose_skins_to_identity() {
        let skeleton = skeleton();
        for matrix in Pose::rest(&skeleton).skin(&skeleton) {
            assert!(matrix.abs_diff_eq(Mat4::IDENTITY, 1e-6), "{matrix}");
        }
    }

    #[test]
    fn moving_a_parent_moves_its_children() {
        let skeleton = skeleton();
        let mut pose = Pose::rest(&skeleton);
        pose.locals[2].t.x = 0.5;
        let world = pose.world(&skeleton);
        let head = world[1].transform_point3(Vec3::ZERO);
        assert!((head - Vec3::new(0.5, 0.4, 0.0)).length() < 1e-6, "{head}");
        let skin = pose.skin(&skeleton);
        let moved = skin[1].transform_point3(Vec3::new(0.0, 0.4, 0.0));
        assert!(
            (moved - Vec3::new(0.5, 0.4, 0.0)).length() < 1e-6,
            "{moved}"
        );
    }

    #[test]
    fn blends_lerp_and_slerp() {
        let a = Transform::IDENTITY;
        let b = Transform::new(
            Vec3::new(2.0, 0.0, 0.0),
            Quat::from_rotation_z(std::f32::consts::FRAC_PI_2),
            Vec3::splat(3.0),
        );
        let half = a.blend(&b, 0.5);
        assert!((half.t.x - 1.0).abs() < 1e-6);
        assert!((half.s - Vec3::splat(2.0)).length() < 1e-6);
        let angle = half.r.angle_between(Quat::IDENTITY);
        assert!(
            (angle - std::f32::consts::FRAC_PI_4).abs() < 1e-5,
            "{angle}"
        );
        // The long way round is never taken: -q is the same rotation as q.
        let flipped = Transform { r: -b.r, ..b };
        let half = a.blend(&flipped, 0.5);
        let angle = half.r.angle_between(Quat::IDENTITY);
        assert!(
            (angle - std::f32::consts::FRAC_PI_4).abs() < 1e-5,
            "{angle}"
        );
    }

    #[test]
    fn sockets_follow_their_joint() {
        let skeleton = skeleton();
        let socket = Socket {
            name: "socket_head".into(),
            joint: 1,
            local: Transform::new(Vec3::new(0.0, 0.2, 0.0), Quat::IDENTITY, Vec3::ONE),
            extras: serde_json::Value::Null,
        };
        let mut pose = Pose::rest(&skeleton);
        pose.locals[0].t.x = 1.0;
        let at = socket_position(&pose.world(&skeleton), &socket);
        assert!((at - Vec3::new(1.0, 0.6, 0.0)).length() < 1e-6, "{at}");
    }
}
