//! Whether the pointer is on the creature as drawn: its meshes skinned on
//! the CPU with the frame's matrices, projected to window points, and the
//! point tested against each triangle. Exact, no GPU, no allocation once
//! sized: cheap enough for every mouse move of a click-through window.

use glam::{Mat4, Vec2, Vec3};

use crate::creature::asset::{CreatureAsset, Primitive};
use crate::creature::frame::Frame;

#[derive(Debug, Default)]
pub struct HitTest {
    /// One primitive's vertices in window points.
    scratch: Vec<Vec2>,
}

impl HitTest {
    /// Whether the window point `p` is on `asset` drawn as `frame`, where
    /// `to_window` takes creature space to window points as drawn
    /// ([`super::Renderer::to_window`]).
    pub fn hits(&mut self, asset: &CreatureAsset, frame: &Frame, to_window: Mat4, p: Vec2) -> bool {
        for primitive in &asset.primitives {
            self.scratch.clear();
            self.scratch.extend((0..primitive.positions.len()).map(|i| {
                to_window
                    .transform_point3(placed(primitive, frame, i))
                    .truncate()
            }));
            let hit = primitive.indices.chunks_exact(3).any(|tri| {
                let corner = |k: usize| self.scratch.get(tri[k] as usize).copied();
                match (corner(0), corner(1), corner(2)) {
                    (Some(a), Some(b), Some(c)) => inside(p, a, b, c),
                    _ => false,
                }
            });
            if hit {
                return true;
            }
        }
        false
    }
}

/// Vertex `i` of `primitive` in creature space on `frame`.
fn placed(primitive: &Primitive, frame: &Frame, i: usize) -> Vec3 {
    let position = primitive.positions[i];
    let (Some(joints), Some(weights)) = (primitive.joints.get(i), primitive.weights.get(i)) else {
        return primitive.world.transform_point3(position);
    };
    let total = weights.element_sum();
    if total <= 1e-4 {
        return position;
    }
    joints
        .iter()
        .zip(weights.to_array())
        .map(|(&joint, weight)| {
            let skin = frame
                .skin
                .get(usize::from(joint))
                .unwrap_or(&Mat4::IDENTITY);
            skin.transform_point3(position) * (weight / total)
        })
        .sum()
}

/// Whether `p` is in the triangle `a b c`, whichever way it winds.
fn inside(p: Vec2, a: Vec2, b: Vec2, c: Vec2) -> bool {
    let side = |u: Vec2, v: Vec2| (v - u).perp_dot(p - u);
    let (x, y, z) = (side(a, b), side(b, c), side(c, a));
    (x >= 0.0 && y >= 0.0 && z >= 0.0) || (x <= 0.0 && y <= 0.0 && z <= 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::motion::{Animator, Placement, Play};
    use crate::render::uniforms::{PITCH_DEGREES, camera};

    const FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/mini_creature.glb");

    /// The fixture's box, 0.6 m wide and 0.7 m tall, stands at x 90 pt on
    /// the window's bottom, 150 pt a meter: 45 pt each side, 105 pt tall.
    fn setup() -> (CreatureAsset, Animator, Placement) {
        let asset = CreatureAsset::from_glb(FIXTURE).unwrap();
        let mut animator = Animator::new(&asset);
        animator.play("idle", Play::Loop, 1.0, 0.0);
        (asset, animator, Placement::new(Vec2::new(90.0, 0.0), 150.0))
    }

    fn to_window(frame: &Frame) -> Mat4 {
        let (model, view) = camera(frame, Vec2::ZERO, 0.15, PITCH_DEGREES);
        view * model
    }

    #[test]
    fn the_middle_of_the_box_hits_and_a_corner_misses() {
        let (asset, mut animator, placement) = setup();
        let frame = animator.update(1.0 / 30.0, &placement);
        let mut hit = HitTest::default();
        let at =
            |hit: &mut HitTest, x, y| hit.hits(&asset, &frame, to_window(&frame), Vec2::new(x, y));
        assert!(at(&mut hit, 90.0, 50.0), "the middle");
        assert!(at(&mut hit, 130.0, 100.0), "near a corner, inside");
        assert!(!at(&mut hit, 140.0, 50.0), "beside it");
        assert!(!at(&mut hit, 90.0, 120.0), "above it");
        assert!(!at(&mut hit, 2.0, 2.0), "the window's corner");
    }

    #[test]
    fn a_moved_bone_moves_where_it_hits() {
        let (asset, mut animator, placement) = setup();
        let mut frame = animator.update(1.0 / 30.0, &placement);
        let mut hit = HitTest::default();
        // Where the top of the box would be, 0.6 m to its right.
        let over = Vec2::new(90.0 + 0.6 * 150.0, 90.0);
        assert!(!hit.hits(&asset, &frame, to_window(&frame), over));
        let head = asset.skeleton.find("head").unwrap();
        animator.advance(0.0);
        animator.pose_mut().locals[head].t.x += 0.6;
        animator.write_frame(&placement, &mut frame);
        assert!(hit.hits(&asset, &frame, to_window(&frame), over));
    }
}
