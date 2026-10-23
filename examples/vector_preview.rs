//! Writes a sheet PNG of the Classic style's vector cat in every pose, one
//! row per coat, the props below, and each coat eating from its bowl where
//! the behaviour puts it:
//! `cargo run --example vector_preview -- out.png [pixels per sprite pixel]`

use meow::sprites::{
    Bubble, CANVAS_H, CANVAS_W, Coat, Facing, Kick, Look, PROP_H, PROP_W, Pose, Prop, SPIT,
};
use meow::style::{self, Style};
use meow::vector_cat::{self, Bitmap};

const BACKGROUND: [u8; 3] = [200, 210, 225];
/// Every other cell a shade darker, to see where each canvas ends.
const BACKGROUND_ALT: [u8; 3] = [188, 199, 216];

/// Paints `bitmap` on a cell of the sheet with its top left corner at
/// (`left`, `top`), the cell's background first.
fn blit(rgb: &mut [u8], w: usize, bitmap: &Bitmap, left: usize, top: usize, alt: bool) {
    let back = if alt { BACKGROUND_ALT } else { BACKGROUND };
    for y in 0..bitmap.height {
        let offset = ((top + y) * w + left) * 3;
        for px in rgb[offset..offset + bitmap.width * 3].chunks_mut(3) {
            px.copy_from_slice(&back);
        }
    }
    over(rgb, w, bitmap, left, top);
}

/// Paints `bitmap` over what the sheet already shows there.
fn over(rgb: &mut [u8], w: usize, bitmap: &Bitmap, left: usize, top: usize) {
    let rgba = bitmap.rgba();
    for y in 0..bitmap.height {
        for x in 0..bitmap.width {
            let px = &rgba[(y * bitmap.width + x) * 4..][..4];
            let a = u32::from(px[3]);
            let offset = ((top + y) * w + left + x) * 3;
            for (out, &v) in rgb[offset..offset + 3].iter_mut().zip(px) {
                *out = ((u32::from(v) * a + u32::from(*out) * (255 - a)) / 255) as u8;
            }
        }
    }
}

fn main() {
    style::set(Style::Classic);
    let mut args = std::env::args().skip(1);
    let path = args.next().unwrap_or_else(|| "vector_cat.png".into());
    let scale: f64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(8.0);
    let dangle = |swing, kick| Pose::Dangle { swing, kick };
    let looks = [
        (Pose::Walk(0), Bubble::None, Facing::Right),
        (Pose::Walk(1), Bubble::None, Facing::Right),
        (Pose::Walk(2), Bubble::None, Facing::Right),
        (Pose::Walk(3), Bubble::None, Facing::Right),
        (Pose::Stand, Bubble::Dots(3), Facing::Right),
        (Pose::Blink, Bubble::Bang, Facing::Right),
        (Pose::Sleep, Bubble::Zzz(0), Facing::Right),
        (Pose::Sleep, Bubble::Zzz(3), Facing::Right),
        (dangle(0, Kick::None), Bubble::None, Facing::Right),
        (dangle(0, Kick::Left), Bubble::None, Facing::Right),
        (dangle(-3, Kick::None), Bubble::None, Facing::Right),
        (dangle(3, Kick::Right), Bubble::Dots(2), Facing::Right),
        (Pose::Fall, Bubble::None, Facing::Right),
        (Pose::Cling(0), Bubble::None, Facing::Right),
        (Pose::Cling(1), Bubble::None, Facing::Right),
        (Pose::Ceiling(1), Bubble::None, Facing::Right),
        (Pose::Blink, Bubble::Hearts(3), Facing::Right),
        (Pose::Blink, Bubble::Hearts(7), Facing::Right),
        (Pose::Walk(0), Bubble::Bang, Facing::Left),
        (Pose::Eat(0), Bubble::None, Facing::Right),
        (Pose::Eat(1), Bubble::None, Facing::Right),
        (Pose::Swat(0), Bubble::None, Facing::Right),
        (Pose::Swat(1), Bubble::None, Facing::Right),
        (Pose::Hack(0), Bubble::None, Facing::Right),
        (Pose::Hack(1), Bubble::None, Facing::Right),
        (Pose::Hack(SPIT), Bubble::None, Facing::Right),
        (Pose::Squat(0), Bubble::None, Facing::Right),
        (Pose::Squat(1), Bubble::None, Facing::Right),
    ];
    let props = [
        Prop::Bowl(3),
        Prop::Bowl(2),
        Prop::Bowl(1),
        Prop::Bowl(0),
        Prop::Mouse(0),
        Prop::Mouse(1),
        Prop::Hairball(3),
        Prop::Hairball(2),
        Prop::Hairball(1),
        Prop::Sparkle(0),
        Prop::Sparkle(1),
        Prop::Poop(3),
        Prop::Poop(2),
        Prop::Poop(1),
    ];
    let px = |units: usize| (units as f64 * scale).round() as usize;
    let (cell_w, row_h) = (px(CANVAS_W), px(CANVAS_H));
    let columns = 14;
    let bands = looks.len().div_ceil(columns);
    let w = cell_w * columns;
    let h = row_h * bands * Coat::ALL.len() + px(PROP_H) + row_h;
    let mut rgb = BACKGROUND.repeat(w * h);
    for (row, &coat) in Coat::ALL.iter().enumerate() {
        for (i, &(pose, bubble, facing)) in looks.iter().enumerate() {
            let look = Look {
                pose,
                bubble,
                facing,
            };
            let bitmap = vector_cat::render_look(&look, coat, scale).expect("bitmap");
            let top = (row * bands + i / columns) * row_h;
            let col = i % columns;
            blit(
                &mut rgb,
                w,
                &bitmap,
                col * cell_w,
                top,
                (col + row) % 2 == 1,
            );
        }
    }
    let props_top = row_h * bands * Coat::ALL.len();
    for (i, &prop) in props.iter().enumerate() {
        let bitmap = vector_cat::render_prop(prop, Facing::Right, scale).expect("bitmap");
        blit(&mut rgb, w, &bitmap, i * px(PROP_W), props_top, i % 2 == 1);
    }
    // Each coat at its bowl: the bowl tucks `BOWL_OVERLAP` (two sprite
    // pixels) under the lowered head, its bottom on the cat's ground.
    let meal_top = props_top + px(PROP_H);
    let meal_w = cell_w + px(PROP_W) - px(2);
    for (i, &coat) in Coat::ALL.iter().enumerate() {
        let left = i * meal_w;
        let ground = Bitmap::new(meal_w, row_h).expect("bitmap");
        blit(&mut rgb, w, &ground, left, meal_top, i % 2 == 1);
        let bowl = vector_cat::render_prop(Prop::Bowl(2), Facing::Right, scale).expect("bowl");
        let eating = Look {
            pose: Pose::Eat(1),
            bubble: Bubble::None,
            facing: Facing::Right,
        };
        let cat = vector_cat::render_look(&eating, coat, scale).expect("bitmap");
        over(&mut rgb, w, &cat, left, meal_top);
        let bowl_top = meal_top + px(CANVAS_H - PROP_H);
        over(&mut rgb, w, &bowl, left + px(CANVAS_W - 2), bowl_top);
    }
    let file = std::fs::File::create(&path).expect("create output file");
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), w as u32, h as u32);
    encoder.set_color(png::ColorType::Rgb);
    encoder
        .write_header()
        .and_then(|mut wr| wr.write_image_data(&rgb))
        .expect("write png");
}
