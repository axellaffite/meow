//! What the animator hands the renderer each frame.
//!
//! 3D positions are in the creature's space (glTF, meters, origin on the
//! ground, Y up, facing +Z), before [`Frame::model`].

use glam::Mat4;

use super::pose::Pose;

/// One frame of the creature, ready to draw.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    /// The joints' local transforms.
    pub pose: Pose,
    /// Skinning matrices (world times inverse bind), one per skin joint.
    pub skin: Vec<Mat4>,
    /// Creature space to window points: the creature's origin placed in its
    /// window (bottom-left origin, y up, z toward the viewer), scaled from
    /// meters to points, mirrored on x when it faces the other way (then the
    /// front face winding flips). The renderer adds the camera.
    pub model: Mat4,
    /// The contact shadow on the ground, if any.
    pub shadow: Option<Shadow>,
}

/// A soft ellipse on the ground under the creature.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadow {
    /// The creature origin's height above the ground, meters: the shadow
    /// stays on the ground, shrinking and fading as it rises.
    pub height: f32,
    /// Where its centre is, creature x, meters.
    pub center_x: f32,
    /// Its half width, meters.
    pub radius: f32,
}

impl Frame {
    /// `from` into `self`, reusing `self`'s buffers (a derived `clone_from`
    /// would allocate them again): nothing allocated once they are sized.
    pub fn copy_from(&mut self, from: &Frame) {
        self.pose.locals.clone_from(&from.pose.locals);
        self.skin.clone_from(&from.skin);
        self.model = from.model;
        self.shadow = from.shadow;
    }
}
