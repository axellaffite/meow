//! Renders a 3D creature offscreen to a PNG sheet, no window needed: eight
//! frames of it idling, then nodding from 0.5 s, over a light desktop (top
//! row, light theme) and a dark one (bottom row, dark theme).
//! `cargo run --example model_preview -- [creature.glb] [out.png]`
//! (the test fixture and model.png by default).

use glam::Vec2;
use meow::creature::CreatureAsset;
use meow::motion::{Animator, Placement, Play};
use meow::render::{Offscreen, Quality, RenderSettings, Renderer, Theme};

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/mini_creature.glb"
);
/// One cell, in points, drawn at 2x.
const CELL: (u32, u32) = (120, 150);
const SCALE: u32 = 2;
const FRAMES: usize = 8;
const LIGHT: [u8; 3] = [236, 238, 242];
const DARK: [u8; 3] = [38, 40, 46];

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let is_model = |path: &str| {
        let path = path.to_ascii_lowercase();
        path.ends_with(".glb") || path.ends_with(".gltf")
    };
    let (model, out) = match args.as_slice() {
        [] => (FIXTURE.to_string(), "model.png".to_string()),
        // A lone model is read, never written over.
        [model] if is_model(model) => (model.clone(), "model.png".to_string()),
        [out] => (FIXTURE.to_string(), out.clone()),
        [model, out, ..] => (model.clone(), out.clone()),
    };
    if is_model(&out) {
        eprintln!("model_preview: {out} looks like a model, not a PNG to write");
        std::process::exit(1);
    }
    if let Err(error) = run(&model, &out) {
        eprintln!("model_preview: {error}");
        std::process::exit(1);
    }
}

fn run(model: &str, out: &str) -> Result<(), String> {
    let asset = CreatureAsset::load(std::path::Path::new(model))?;
    let (cell_w, cell_h) = (CELL.0 * SCALE, CELL.1 * SCALE);
    let offscreen = Offscreen::new(cell_w, cell_h)?;
    let mut renderer = Renderer::new(offscreen.device(), &asset)?;
    let names: Vec<&str> = asset.clips.iter().map(|c| c.name.as_str()).collect();
    println!(
        "{model}: {} joints, clips {names:?}",
        asset.skeleton.joints.len()
    );

    let (sheet_w, sheet_h) = (cell_w * FRAMES as u32, cell_h * 2);
    let mut sheet = vec![0u8; (sheet_w * sheet_h * 4) as usize];
    for (row, (theme, background)) in [(Theme::Light, LIGHT), (Theme::Dark, DARK)]
        .into_iter()
        .enumerate()
    {
        let mut animator = Animator::new(&asset);
        // The first clip, else idle; the second, else nod.
        let first = names.first().copied().unwrap_or("idle");
        let second = names.get(1).copied().unwrap_or("nod");
        animator.play(first, Play::Loop, 1.0, 0.0);
        let mut placement = Placement::new(Vec2::new(CELL.0 as f32 / 2.0, 14.0), 120.0);
        // Turned a little, to show its side.
        placement.yaw = 0.5;
        let mut frame = animator.update(0.0, &placement);
        let mut time = 0.0;
        let mut gpu_ms = 0.0;
        for column in 0..FRAMES {
            if column > 0 {
                // A tenth of a second between frames, at 30 fps.
                for _ in 0..3 {
                    time += 1.0 / 30.0;
                    if (time - 0.5f32).abs() < 1.0 / 60.0 {
                        animator.play(second, Play::Loop, 1.0, 0.25);
                    }
                    animator.update_into(1.0 / 30.0, &placement, &mut frame);
                }
            }
            let settings = RenderSettings {
                theme,
                quality: Quality::Balanced,
                time,
                scale: SCALE as f32,
                ..RenderSettings::default()
            };
            let rendered = offscreen.render(&mut renderer, &frame, &settings, true)?;
            gpu_ms += rendered.gpu_ms;
            paste(
                &mut sheet,
                sheet_w,
                &rendered.rgba,
                (cell_w, cell_h),
                (column as u32 * cell_w, row as u32 * cell_h),
                background,
            );
        }
        println!(
            "{theme:?}: {:.2} ms of GPU a frame at {cell_w} x {cell_h} px",
            gpu_ms / FRAMES as f64
        );
    }
    write_png(out, sheet_w, sheet_h, &sheet)?;
    println!("wrote {out}");
    Ok(())
}

/// Lays straight-alpha `rgba` over `background` at `at` on the sheet.
fn paste(
    sheet: &mut [u8],
    sheet_w: u32,
    rgba: &[u8],
    (w, h): (u32, u32),
    at: (u32, u32),
    background: [u8; 3],
) {
    for y in 0..h {
        for x in 0..w {
            let src = &rgba[((y * w + x) * 4) as usize..][..4];
            let a = u32::from(src[3]);
            let dst = &mut sheet[(((at.1 + y) * sheet_w + at.0 + x) * 4) as usize..][..4];
            for c in 0..3 {
                dst[c] =
                    ((u32::from(src[c]) * a + u32::from(background[c]) * (255 - a)) / 255) as u8;
            }
            dst[3] = 255;
        }
    }
}

fn write_png(path: &str, width: u32, height: u32, rgba: &[u8]) -> Result<(), String> {
    let fail = |e: &dyn std::fmt::Display| format!("{path}: {e}");
    let file = std::fs::File::create(path).map_err(|e| fail(&e))?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| fail(&e))?;
    writer.write_image_data(rgba).map_err(|e| fail(&e))
}
