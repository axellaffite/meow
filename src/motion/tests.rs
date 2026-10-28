use super::*;

const FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/mini_creature.glb");

fn asset() -> CreatureAsset {
    CreatureAsset::from_glb(FIXTURE).unwrap()
}

fn placement() -> Placement {
    Placement::new(Vec2::new(90.0, 20.0), 150.0)
}

/// The frame after `seconds`, from a fresh animator playing idle that
/// crossfades into nod, at `fps`.
fn run(fps: f32, seconds: f32) -> Frame {
    let mut animator = Animator::new(&asset());
    assert!(animator.play("idle", Play::Loop, 1.0, 0.0));
    assert!(animator.play("nod", Play::Loop, 1.0, 0.3));
    // The frame the clips come in, then `seconds`.
    let mut frame = animator.update(1.0 / fps, &placement());
    for _ in 0..(seconds * fps).round() as usize {
        animator.update_into(1.0 / fps, &placement(), &mut frame);
    }
    frame
}

#[test]
fn the_same_clips_give_the_same_pose_at_30_and_120_fps() {
    for seconds in [0.1, 0.2, 0.5] {
        let (slow, fast) = (run(30.0, seconds), run(120.0, seconds));
        for (a, b) in slow.skin.iter().zip(&fast.skin) {
            assert!(a.abs_diff_eq(*b, 1e-4), "{seconds} s: {a} against {b}");
        }
    }
}

#[test]
fn a_once_clip_plays_to_its_end_before_the_next_fades_in() {
    let mut animator = Animator::new(&asset());
    assert!(animator.play("nod", Play::Once, 1.0, 0.0));
    let mut frame = animator.update(1.0 / 30.0, &placement());
    // Asked for while nod (20 frames, 0.67 s) plays: idle waits for its end.
    assert!(animator.play("idle", Play::Loop, 1.0, 0.1));
    for _ in 0..15 {
        animator.update_into(1.0 / 30.0, &placement(), &mut frame);
    }
    assert_eq!(animator.weight_of("nod"), 1.0, "half way, still nodding");
    assert_eq!(animator.weight_of("idle"), 0.0);
    for _ in 0..12 {
        animator.update_into(1.0 / 30.0, &placement(), &mut frame);
    }
    assert_eq!(animator.weight_of("idle"), 1.0, "nod done, idle in");
    assert!(
        !animator.play("dance", Play::Loop, 1.0, 0.1),
        "no such clip"
    );
}

#[test]
fn mirroring_flips_the_model_determinant() {
    let mut placed = placement();
    assert!(placed.model().determinant() > 0.0);
    placed.mirror = true;
    assert!(placed.model().determinant() < 0.0);
    // The origin lands where it is placed, one meter up is 150 points up.
    let up = placed.model().transform_point3(Vec3::Y);
    assert!((up - Vec3::new(90.0, 170.0, 0.0)).length() < 1e-4, "{up}");
}

#[test]
fn procedural_layers_go_on_top_of_the_clips() {
    let asset = asset();
    let mut animator = Animator::new(&asset);
    animator.play("idle", Play::Loop, 1.0, 0.0);
    let mut frame = animator.update(1.0 / 30.0, &placement());
    let head = asset.skeleton.find("head").unwrap();
    animator.advance(0.0);
    animator.pose_mut().locals[head].t.x += 0.1;
    // A broken layer falls back to rest, never a NaN on screen.
    animator.pose_mut().locals[0].t.y = f32::NAN;
    animator.write_frame(&placement(), &mut frame);
    let moved = frame.skin[head].transform_point3(Vec3::new(0.0, 0.6, 0.0));
    assert!((moved.x - 0.1).abs() < 1e-4, "{moved}");
    assert!(frame.skin.iter().all(|m| m.is_finite()));
    assert!(frame.shadow.is_some_and(|s| (s.radius - 0.33).abs() < 1e-3));
}
