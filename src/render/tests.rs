use super::*;
use crate::motion::{Animator, Placement, Play};

const FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/mini_creature.glb");

/// The fixture idling in a 64 x 80 point window, 50 points a meter.
fn frame(asset: &CreatureAsset, mirror: bool) -> Frame {
    let mut animator = Animator::new(asset);
    animator.play("idle", Play::Loop, 1.0, 0.0);
    let mut placement = Placement::new(Vec2::new(32.0, 4.0), 50.0);
    placement.mirror = mirror;
    animator.update(1.0 / 30.0, &placement)
}

/// Encodes a pass that only clears `target` to `color`.
fn clear(
    command_buffer: &ProtocolObject<dyn MTLCommandBuffer>,
    target: &ProtocolObject<dyn MTLTexture>,
    color: MTLClearColor,
) -> Result<(), String> {
    let pass = MTLRenderPassDescriptor::new();
    // SAFETY: attachment 0 always exists.
    let attachment = unsafe { pass.colorAttachments().objectAtIndexedSubscript(0) };
    attachment.setTexture(Some(target));
    attachment.setLoadAction(MTLLoadAction::Clear);
    attachment.setStoreAction(MTLStoreAction::Store);
    attachment.setClearColor(color);
    let encoder = command_buffer
        .renderCommandEncoderWithDescriptor(&pass)
        .ok_or("could not start a render pass")?;
    encoder.endEncoding();
    Ok(())
}

#[test]
fn renders_offscreen_transparent_around_the_creature() {
    if MTLCreateSystemDefaultDevice().is_none() {
        eprintln!("no Metal device: skipped");
        return;
    }
    let asset = CreatureAsset::from_glb(FIXTURE).unwrap();
    let (width, height) = (64u32, 80u32);
    for mirror in [false, true] {
        for theme in [Theme::Light, Theme::Dark] {
            for quality in [Quality::Battery, Quality::Balanced] {
                let settings = RenderSettings {
                    theme,
                    quality,
                    ..RenderSettings::default()
                };
                let rgba =
                    render_rgba(&asset, &frame(&asset, mirror), &settings, width, height).unwrap();
                assert_eq!(rgba.len(), (width * height * 4) as usize);
                let at = |x: u32, y_up: u32| {
                    let o = (((height - 1 - y_up) * width + x) * 4) as usize;
                    [rgba[o], rgba[o + 1], rgba[o + 2], rgba[o + 3]]
                };
                let case = format!("{theme:?} {quality:?} mirrored {mirror}");
                // The fur, opaque and orange (more red than blue), in the
                // middle of the box (0.35 m up is 17.5 pt over the ground).
                let fur = at(32, 4 + 17);
                assert_eq!(fur[3], 255, "{case}: the box is drawn");
                assert!(fur[0] > fur[2] + 40, "{case}: fur colour {fur:?}");
                // Nothing in the corners, nor above the head.
                for (x, y) in [(0, 0), (63, 79), (0, 79), (32, 70)] {
                    assert_eq!(at(x, y), [0, 0, 0, 0], "{case}: empty at {x}, {y}");
                }
            }
        }
    }
}

#[test]
fn the_contact_shadow_shows_in_front_of_the_feet() {
    if MTLCreateSystemDefaultDevice().is_none() {
        eprintln!("no Metal device: skipped");
        return;
    }
    let asset = CreatureAsset::from_glb(FIXTURE).unwrap();
    let mut shadowed = frame(&asset, false);
    let settings = RenderSettings::default();
    // The most cover under the ground line (4 pt up), where the box is not.
    let alpha_at = |frame: &Frame| {
        let rgba = render_rgba(&asset, frame, &settings, 64, 80).unwrap();
        (24..40)
            .flat_map(|x| (0..4).map(move |y_up| (x, y_up)))
            .map(|(x, y_up)| rgba[(((80 - 1 - y_up) * 64 + x) * 4 + 3) as usize])
            .max()
            .unwrap_or(0)
    };
    assert!(alpha_at(&shadowed) > 0, "a soft shadow");
    shadowed.shadow = None;
    assert_eq!(alpha_at(&shadowed), 0, "none asked");
}

#[test]
fn the_target_is_read_back_from_the_gpu_in_rgba_order() {
    let Ok(offscreen) = Offscreen::new(4, 2) else {
        eprintln!("no Metal device: skipped");
        return;
    };
    // Opaque orange, so a buffer left as it was (zeros) cannot pass.
    let orange = MTLClearColor {
        red: 1.0,
        green: 0.5,
        blue: 0.0,
        alpha: 1.0,
    };
    let command_buffer = offscreen.queue.commandBuffer().unwrap();
    clear(&command_buffer, &offscreen.target, orange).unwrap();
    let blit = command_buffer.blitCommandEncoder().unwrap();
    blit.synchronizeResource(ProtocolObject::from_ref(&*offscreen.target));
    blit.endEncoding();
    command_buffer.commit();
    command_buffer.waitUntilCompleted();
    let bgra = read_target(&offscreen.target, 4, 2).unwrap();
    assert_eq!(bgra.len(), 4 * 2 * 4);
    for px in bgra.chunks_exact(4) {
        assert_eq!(px, [0, 128, 255, 255], "B, G, R, A");
    }
    assert_eq!(straight_rgba(&bgra)[..4], [255, 128, 0, 255]);
}

#[test]
fn premultiplied_pixels_are_unmultiplied() {
    // BGRA premultiplied: half-transparent pure red is (0, 0, 128, 128).
    assert_eq!(straight_rgba(&[0, 0, 128, 128]), [255, 0, 0, 128]);
    assert_eq!(straight_rgba(&[9, 9, 9, 0]), [0, 0, 0, 0]);
}

#[test]
fn an_empty_target_is_an_error() {
    let asset = CreatureAsset::from_glb(FIXTURE).unwrap();
    let frame = frame(&asset, false);
    assert!(render_rgba(&asset, &frame, &RenderSettings::default(), 0, 10).is_err());
}

/// Each window gets a share of one compiled renderer: it draws the same
/// picture, with its own targets, at its own size.
#[test]
fn a_shared_renderer_draws_the_same_picture() {
    let Ok(small) = Offscreen::new(64, 80) else {
        eprintln!("no Metal device: skipped");
        return;
    };
    let retina = Offscreen::new(128, 160).unwrap();
    let asset = CreatureAsset::from_glb(FIXTURE).unwrap();
    let frame = frame(&asset, false);
    let settings = RenderSettings::default();
    let mut first = Renderer::new(small.device(), &asset).unwrap();
    let mut second = first.share();
    let draw = |offscreen: &Offscreen, renderer: &mut Renderer, settings: &RenderSettings| {
        offscreen
            .render(renderer, &frame, settings, true)
            .unwrap()
            .rgba
    };
    let a = draw(&small, &mut first, &settings);
    assert_eq!(draw(&small, &mut second, &settings), a);
    // A Retina window, twice the pixels for the same points, does not
    // disturb the other.
    let double = RenderSettings {
        scale: 2.0,
        ..settings
    };
    let big = draw(&retina, &mut second, &double);
    assert_eq!(big.len(), 128 * 160 * 4);
    assert_eq!(big[((159 - 42) * 128 + 64) * 4 + 3], 255, "the same place");
    assert_eq!(draw(&small, &mut first, &settings), a);
}

#[test]
fn qualities_have_names() {
    assert_eq!(Quality::from_name(" Battery "), Some(Quality::Battery));
    assert_eq!(Quality::from_name("balanced"), Some(Quality::Balanced));
    assert_eq!(Quality::from_name("ultra"), None);
}
