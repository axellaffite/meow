//! The motion engine: plays a creature's clips and hands the renderer one
//! [`Frame`] per tick.
//!
//! Each frame:
//! 1. the base layer and the overlay advance (`mixer`): crossfades between
//!    clips, a new clip waiting for a one-shot to end before it fades in;
//! 2. they are sampled and blended into a pose, discrete data bones kept
//!    whole;
//! 3. the caller may add its own procedural layers to that pose
//!    ([`Animator::pose_mut`], with [`spring::Spring`] and the [`rig`]
//!    helpers);
//! 4. the pose becomes world and skinning matrices, and the frame.
//!
//! Every step is frame-rate independent: clips and crossfades run on
//! seconds, a change starts at the end of the frame it comes in, and springs
//! are solved exactly (cut into steps of at most 1/240 s), so 1/120 and
//! 1/12 s frames move alike. Nothing panics on a poor asset: missing clips
//! sample the rest pose. A steady tick allocates nothing.

pub mod mixer;
pub mod rig;
pub mod spring;

use glam::{Mat4, Vec2, Vec3};

use crate::creature::asset::{CreatureAsset, Skeleton};
use crate::creature::frame::{Frame, Shadow};
use crate::creature::pose::Pose;

pub use mixer::{ClipId, Play};
use mixer::{Library, Mixer, Track};

/// Where the creature stands in its window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    /// Its origin (on the ground), window points from the bottom-left corner.
    pub origin: Vec2,
    /// How big it is drawn: one meter is this many points.
    pub points_per_meter: f32,
    /// Facing the other way: mirrored on x (its `_L` side drawn on the right).
    pub mirror: bool,
    /// A turn about its vertical axis, radians (0 faces the viewer).
    pub yaw: f32,
}

impl Placement {
    pub fn new(origin: Vec2, points_per_meter: f32) -> Self {
        Self {
            origin,
            points_per_meter,
            mirror: false,
            yaw: 0.0,
        }
    }

    /// Creature space (meters) to window points.
    pub fn model(&self) -> Mat4 {
        let ppm = if self.points_per_meter.is_finite() {
            self.points_per_meter.max(1e-3)
        } else {
            1.0
        };
        let flip = if self.mirror { -1.0 } else { 1.0 };
        let yaw = if self.yaw.is_finite() { self.yaw } else { 0.0 };
        Mat4::from_translation(self.origin.extend(0.0))
            * Mat4::from_scale(Vec3::new(flip * ppm, ppm, ppm))
            * Mat4::from_rotation_y(yaw)
    }
}

/// Plays one creature's clips.
#[derive(Clone, Debug)]
pub struct Animator {
    skeleton: Skeleton,
    library: Library,
    mixer: Mixer,
    rest: Pose,
    pose: Pose,
    /// World matrices, reused each frame.
    world: Vec<Mat4>,
    /// The contact shadow each frame carries, from the mesh's width at rest;
    /// set it to change it, `None` for none.
    pub shadow: Option<Shadow>,
}

impl Animator {
    pub fn new(asset: &CreatureAsset) -> Self {
        let rest = Pose::rest(&asset.skeleton);
        Self {
            skeleton: asset.skeleton.clone(),
            library: Library::new(asset),
            mixer: Mixer::new(&rest),
            pose: rest.clone(),
            rest,
            world: Vec::with_capacity(asset.skeleton.joints.len()),
            shadow: rest_shadow(asset),
        }
    }

    pub fn skeleton(&self) -> &Skeleton {
        &self.skeleton
    }

    pub fn library(&self) -> &Library {
        &self.library
    }

    /// Makes the data bone `bone` discrete (see [`Library::set_discrete`]);
    /// false when there is no such bone.
    pub fn set_discrete(&mut self, bone: &str) -> bool {
        let Some(joint) = self.skeleton.find(bone) else {
            return false;
        };
        self.library.set_discrete(joint);
        true
    }

    /// Plays the clip `name` on the base layer, crossfading over `fade`
    /// seconds. A one-shot playing is let finish first: the new clip fades
    /// in as it ends, so `play("wave", Once)` then `play("idle", Loop)`
    /// waves, then idles. False when the asset has no such clip.
    pub fn play(&mut self, name: &str, play: Play, speed: f32, fade: f32) -> bool {
        let Some(id) = self.library.find(name) else {
            return false;
        };
        let library = &self.library;
        let current = self.mixer.base.current().filter(|t| t.is_incoming());
        if current.is_some_and(|t| t.clip == id && t.play == Play::Loop && play == Play::Loop) {
            return true;
        }
        // Wait for the one-shot on, or with the clip already waiting for it.
        let delay = current.map_or(0.0, |t| t.remaining(library));
        let track = Track::new(id, play, speed, start_of(library, id, play), fade);
        self.mixer.base.push_after(track, fade, delay);
        true
    }

    /// Plays the clip `name` over the base, on the joints `mask` weighs
    /// ([`Mixer::mask_below`]; `None` for all), fading in over `fade`.
    pub fn overlay(&mut self, name: &str, play: Play, fade: f32, mask: Option<Vec<f32>>) -> bool {
        let Some(id) = self.library.find(name) else {
            return false;
        };
        let start = start_of(&self.library, id, play);
        self.mixer.mask = mask;
        self.mixer
            .overlay
            .push(Track::new(id, play, 1.0, start, fade), fade);
        true
    }

    /// Fades the overlay out over `fade` seconds.
    pub fn stop_overlay(&mut self, fade: f32) {
        self.mixer.overlay.stop(fade);
    }

    /// How much of the clip `name` shows, 0 to 1, base and overlay together.
    pub fn weight_of(&self, name: &str) -> f32 {
        self.library.find(name).map_or(0.0, |id| {
            (self.mixer.base.weight_of(id) + self.mixer.overlay.weight_of(id)).min(1.0)
        })
    }

    /// Whether the base's newest clip is a one-shot played to its end.
    pub fn is_done(&self) -> bool {
        self.mixer
            .base
            .current()
            .is_some_and(|t| t.done(&self.library))
    }

    /// Advances the clips `dt` seconds and blends them into the pose.
    pub fn advance(&mut self, dt: f32) {
        let dt = if dt.is_finite() {
            dt.clamp(0.0, 0.25)
        } else {
            0.0
        };
        self.mixer.base.advance(dt, &self.library);
        self.mixer.overlay.advance(dt, &self.library);
        self.mixer
            .evaluate(&self.library, &self.rest, &mut self.pose);
    }

    /// The pose [`Animator::advance`] blended: add procedural layers here,
    /// before [`Animator::write_frame`].
    pub fn pose_mut(&mut self) -> &mut Pose {
        &mut self.pose
    }

    /// The pose placed by `placement`, into `frame` (its buffers reused).
    pub fn write_frame(&mut self, placement: &Placement, frame: &mut Frame) {
        for (local, rest) in self.pose.locals.iter_mut().zip(&self.rest.locals) {
            *local = rig::sane(*local, *rest);
        }
        rig::world_into(&self.pose, &self.skeleton, &mut self.world);
        frame.pose.locals.clone_from(&self.pose.locals);
        let joints = &self.skeleton.joints[..self.skeleton.skin_joints];
        frame.skin.clear();
        frame.skin.extend(
            joints
                .iter()
                .zip(&self.world)
                .map(|(joint, world)| *world * joint.inverse_bind),
        );
        frame.model = placement.model();
        frame.shadow = self.shadow;
    }

    /// [`Animator::advance`] then [`Animator::write_frame`]: nothing is
    /// allocated once `frame`'s buffers are sized.
    pub fn update_into(&mut self, dt: f32, placement: &Placement, frame: &mut Frame) {
        self.advance(dt);
        self.write_frame(placement, frame);
    }

    /// A new frame `dt` seconds on.
    pub fn update(&mut self, dt: f32, placement: &Placement) -> Frame {
        let mut frame = Frame {
            pose: Pose { locals: Vec::new() },
            skin: Vec::new(),
            model: Mat4::IDENTITY,
            shadow: None,
        };
        self.update_into(dt, placement, &mut frame);
        frame
    }
}

/// Where a track of `id` starts: the end when it plays backwards.
fn start_of(library: &Library, id: ClipId, play: Play) -> f32 {
    match play {
        Play::Reverse => library.duration(id),
        _ => 0.0,
    }
}

/// A shadow as wide as the meshes at rest, under their middle.
fn rest_shadow(asset: &CreatureAsset) -> Option<Shadow> {
    let (mut low, mut high) = (f32::MAX, f32::MIN);
    for primitive in &asset.primitives {
        for &p in &primitive.positions {
            let x = if primitive.is_skinned() {
                p.x
            } else {
                primitive.world.transform_point3(p).x
            };
            low = low.min(x);
            high = high.max(x);
        }
    }
    (high > low).then(|| Shadow {
        height: 0.0,
        center_x: (low + high) / 2.0,
        radius: (high - low) / 2.0 * 1.1,
    })
}

#[cfg(test)]
mod tests;
