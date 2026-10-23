//! Pixel-art cat, drawn as text grids and composed into RGB frames.
//!
//! The cat is drawn facing right; left-facing frames are mirrored. Two coats
//! are other animals, Dédé a pig and a corgi: same canvas and poses, their
//! own head, body and tail.

mod coat;
mod corgi;
mod hairball;
mod hanging;
mod pig;
mod play;

pub use coat::{Animal, Coat, Palette};
pub use hairball::SPIT;
pub use hanging::{Kick, SCRUFF};
pub use play::{PROP_H, PROP_W, Prop, SWAT_PAW, render_prop};

/// Screen points per sprite pixel.
pub const SCALE: f64 = 3.0;
/// Canvas pixel (column, row) at the front of the jaw of the standing and
/// walking cat, where it carries things.
pub const MOUTH: (usize, usize) = (22, BUBBLE_H + HEAD_TOP.len() + 2);

pub const CANVAS_W: usize = 24;
pub const BUBBLE_H: usize = 8;
pub const CAT_H: usize = 14;
pub const CANVAS_H: usize = BUBBLE_H + CAT_H;

pub(crate) const BUBBLE_X: usize = 14;
/// Canvas column of the bubble's tail, just above the head of a cat facing right.
pub const BUBBLE_TAIL_X: usize = BUBBLE_X + 2;
const HEART: [&str; 4] = [".r.r.", "rrrrr", ".rrr.", "..r.."];
/// Left column of each rising heart, above the cat's head and back.
pub(crate) const HEART_LANES: [isize; 3] = [7, 12, 17];
pub(crate) const HEART_STAGGER: usize = 4;
/// Frames for a heart to rise from below the space to above it.
pub(crate) const HEART_CYCLE: usize = BUBBLE_H + 4;
/// The sleeping cat lies lower than the standing one; this is its top row.
pub(crate) const SLEEP_TOP: usize = BUBBLE_H + 3;
const Z: [&str; 5] = ["kkk", "..k", ".k.", "k..", "kkk"];
/// Left column of a z as it appears, between the sleeping cat's ears.
pub(crate) const Z_X: isize = 17;
/// Frames for a z to float from just above the sleeping cat's head out
/// through the top, plus one gone so the two z's never touch.
pub(crate) const Z_CYCLE: usize = SLEEP_TOP + 1;
const LEG_XS: [usize; 4] = [3, 6, 14, 17];
const LEG_ROW: usize = 10;
const FALL_STRIDES: [isize; 4] = [-1, -1, 1, 1];
/// Where the turned cat sits: its feet end on the last column.
const CLING_X: isize = (CANVAS_W - CAT_H) as isize;
const CLING_TOP: usize = CANVAS_H;
const WALK_STRIDES: [[isize; 4]; 4] = [[1, -1, -1, 1], [0; 4], [-1, 1, 1, -1], [0; 4]];

pub type Rgb = [u8; 3];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Facing {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pose {
    Walk(u8),
    Stand,
    Blink,
    Sleep,
    /// Held by the scruff, swung `swing` pixels sideways at the feet.
    Dangle {
        swing: i8,
        kick: Kick,
    },
    /// Mid-air, legs spread.
    Fall,
    /// Stuck to a wall: upright, paws against the side it faces; the phase
    /// moves the paws like walking, for scrabbling.
    Cling(u8),
    /// Stuck to the top of the screen, upside down, paws up; the phase
    /// scrabbles them like `Cling`.
    Ceiling(u8),
    /// Head down in the bowl, eyes shut; the bite bobs the head.
    Eat(u8),
    /// Batting at something with a front paw; the phase raises or strikes.
    Swat(u8),
    /// Hunched over, coughing up a hairball: 0 catches its breath, 1 heaves,
    /// `SPIT` brings it up.
    Hack(u8),
    /// Rump lowered, tail up, pooping: 0 settles, 1 pushes, `SPIT` lets go.
    Squat(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bubble {
    None,
    Dots(u8),
    Bang,
    /// Small z's floating up from the sleeping cat; the frame drives the animation.
    Zzz(u8),
    /// Small hearts rising above the cat; the frame drives the animation.
    Hearts(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Look {
    pub pose: Pose,
    pub bubble: Bubble,
    pub facing: Facing,
}

/// Row-major pixels, `None` is transparent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<Option<Rgb>>,
}

impl Frame {
    pub fn get(&self, x: usize, y: usize) -> Option<Rgb> {
        self.pixels[y * self.width + x]
    }
}

// Art cells: `k` outline, `w` white, `p` pink, `r` red; the others name
// parts of the cat (fur, stripes, blaze, muzzle, legs, tail, eyes) that each
// coat colors its own way, see `Coat::color`.
const HEAD_TOP: [&str; 4] = [
    concat!("..............", "k", "......", "k"),
    concat!(".kk", "..........", "kpk", "....", "kpk"),
    concat!("kttk", ".........", "k", "oo", "kkkk", "oo", "k"),
    concat!("ktk", "..........", "k", "ooo", "mm", "ooo", "k"),
];

const EYES_OPEN: [&str; 2] = [
    concat!(
        "ktk",
        "..........",
        "k",
        "o",
        "e",
        "o",
        "mm",
        "o",
        "f",
        "o",
        "k"
    ),
    concat!(
        ".ktk",
        ".........",
        "k",
        "o",
        "e",
        "c",
        "pp",
        "c",
        "f",
        "o",
        "k"
    ),
];

const EYES_CLOSED: [&str; 2] = [
    concat!("ktk", "..........", "k", "ooo", "mm", "ooo", "k"),
    concat!(".ktk", ".........", "k", "o", "kk", "pp", "kk", "o", "k"),
];

const BODY: [&str; 4] = [
    concat!("..kt", "kkkkkkkkk", "k", "o", "wwwwww", "o", "k"),
    concat!("..k", "ossoossooss", "wwwww", "kkk"),
    concat!("..k", "oooooooooooo", "wwww", "k"),
    concat!("..k", "oooooooooooooooo", "k"),
];

const BUBBLE_FRAME: [&str; 7] = [
    ".kkkkkkk.",
    "kwwwwwwwk",
    "kwwwwwwwk",
    "kwwwwwwwk",
    "kwwwwwwwk",
    ".kkkkkkk.",
    "..k......",
];

const BANG: [&str; 4] = ["kwwwkwwwk", "kwwwkwwwk", "kwwwwwwwk", "kwwwkwwwk"];

/// A mutable char grid used while composing a frame.
struct Grid {
    cells: Vec<char>,
    /// The animal drawn.
    animal: Animal,
}

impl Grid {
    fn new(animal: Animal) -> Self {
        Self {
            cells: vec!['.'; CANVAS_W * CANVAS_H],
            animal,
        }
    }

    /// An empty grid for the same animal, to compose a pose in.
    fn blank(&self) -> Self {
        Self::new(self.animal)
    }

    fn set(&mut self, x: isize, y: usize, c: char) {
        if (0..CANVAS_W as isize).contains(&x) && y < CANVAS_H {
            self.cells[y * CANVAS_W + x as usize] = c;
        }
    }

    fn blit(&mut self, rows: &[&str], x0: usize, y0: usize) {
        for (dy, row) in rows.iter().enumerate() {
            for (dx, c) in row.chars().enumerate() {
                if c != '.' {
                    self.set((x0 + dx) as isize, y0 + dy, c);
                }
            }
        }
    }

    /// Like `blit`, at a position that may be off the canvas, keeping only
    /// the rows above `floor`.
    fn blit_above(&mut self, rows: &[&str], x0: isize, y0: isize, floor: usize) {
        for (dy, row) in rows.iter().enumerate() {
            let y = y0 + dy as isize;
            if !(0..floor as isize).contains(&y) {
                continue;
            }
            for (dx, c) in row.chars().enumerate() {
                if c != '.' {
                    self.set(x0 + dx as isize, y as usize, c);
                }
            }
        }
    }
}

fn cat_rows(animal: Animal, eyes_closed: bool) -> Vec<&'static str> {
    match animal {
        Animal::Pig => return pig::rows(eyes_closed),
        Animal::Corgi => return corgi::rows(eyes_closed),
        Animal::Cat => {}
    }
    let eyes = if eyes_closed { EYES_CLOSED } else { EYES_OPEN };
    HEAD_TOP
        .iter()
        .chain(eyes.iter())
        .chain(BODY.iter())
        .copied()
        .collect()
}

fn draw_belly(grid: &mut Grid, y: usize) {
    for x in 2..=19 {
        grid.set(x, y, 'k');
    }
}

/// Draws the belly line at `top + LEG_ROW` and the legs below it, `extra` rows longer than usual.
fn draw_legs(grid: &mut Grid, top: usize, strides: [isize; 4], extra: usize) {
    let y = top + LEG_ROW;
    let shifted = y + 2 + extra;
    draw_belly(grid, y);
    // Outlines first so neighbouring legs never cover each other's fur.
    for (&x, &dx) in LEG_XS.iter().zip(&strides) {
        let x = x as isize;
        for row in y + 1..shifted {
            grid.set(x - 1, row, 'k');
            grid.set(x + 2, row, 'k');
        }
        grid.set(x - 1 + dx, shifted, 'k');
        grid.set(x + 2 + dx, shifted, 'k');
        for fx in (x - 1 + dx)..=(x + 2 + dx) {
            grid.set(fx, shifted + 1, 'k');
        }
    }
    for (&x, &dx) in LEG_XS.iter().zip(&strides) {
        let x = x as isize;
        let rows = (y..shifted).map(|row| (row, 0)).chain([(shifted, dx)]);
        for (row, shift) in rows {
            grid.set(x + shift, row, 'l');
            grid.set(x + 1 + shift, row, 'l');
        }
    }
}

fn draw_standing(grid: &mut Grid, eyes_closed: bool, strides: [isize; 4]) {
    match grid.animal {
        Animal::Pig => return pig::draw_standing(grid, eyes_closed, strides),
        Animal::Corgi => return corgi::draw_standing(grid, eyes_closed, strides),
        Animal::Cat => {}
    }
    grid.blit(&cat_rows(grid.animal, eyes_closed), 0, BUBBLE_H);
    draw_legs(grid, BUBBLE_H, strides, 0);
}

/// The standing cat turned a quarter turn: head up, paws reaching the right
/// edge of the canvas (the wall), tail hanging below.
fn draw_clinging(grid: &mut Grid, strides: [isize; 4]) {
    let mut standing = grid.blank();
    draw_standing(&mut standing, false, strides);
    for y in BUBBLE_H..CANVAS_H {
        for x in 0..CANVAS_W {
            let c = standing.cells[y * CANVAS_W + x];
            if c == '.' {
                continue;
            }
            // Legs point right, toward the wall; the tail tip falls off the bottom.
            let new_x = (y - BUBBLE_H) as isize + CLING_X;
            let new_y = CLING_TOP as isize - x as isize;
            if new_y >= 0 {
                grid.set(new_x, new_y as usize, c);
            }
        }
    }
}

/// The standing cat flipped over: paws on the top edge of the canvas (the
/// top of the screen), tail hanging down.
fn draw_upside_down(grid: &mut Grid, strides: [isize; 4]) {
    let mut standing = grid.blank();
    draw_standing(&mut standing, false, strides);
    for y in BUBBLE_H..CANVAS_H {
        for x in 0..CANVAS_W {
            let c = standing.cells[y * CANVAS_W + x];
            if c != '.' {
                grid.set(x as isize, CANVAS_H - 1 - y, c);
            }
        }
    }
}

fn draw_sleeping(grid: &mut Grid) {
    let top = SLEEP_TOP;
    grid.blit(&cat_rows(grid.animal, true), 0, top);
    if grid.animal == Animal::Cat {
        tuck_tail(grid, top);
    }
    draw_belly(grid, top + HEAD_TOP.len() + 2 + BODY.len());
}

/// Tucks the sleeping cat's tail away: clears it and closes the back outline.
fn tuck_tail(grid: &mut Grid, top: usize) {
    for y in top + 1..top + 6 {
        for x in 0..4 {
            grid.set(x, y, '.');
        }
    }
    grid.set(3, top + 6, 'k');
}

/// Hearts rise through the space above the cat in three staggered lanes,
/// appearing from below and leaving through the top.
fn draw_hearts(grid: &mut Grid, frame: u8) {
    for (lane, &x) in HEART_LANES.iter().enumerate() {
        let progress = (usize::from(frame) + lane * HEART_STAGGER) % HEART_CYCLE;
        let top = BUBBLE_H as isize - progress as isize;
        let wobble = (progress / 4 % 2) as isize;
        grid.blit_above(&HEART, x + wobble, top, BUBBLE_H);
    }
}

/// Two z's take turns floating up from between the sleeping cat's ears,
/// drifting back over its body and leaving through the top.
fn draw_zs(grid: &mut Grid, frame: u8) {
    for offset in [0, Z_CYCLE / 2] {
        let progress = (usize::from(frame) + offset) % Z_CYCLE;
        let top = (SLEEP_TOP - Z.len()) as isize - progress as isize;
        let drift = (progress / 2) as isize;
        grid.blit_above(&Z, Z_X - drift, top, SLEEP_TOP);
    }
}

fn draw_bubble(grid: &mut Grid, bubble: Bubble) {
    let content: Vec<String> = match bubble {
        Bubble::None => return,
        Bubble::Hearts(frame) => return draw_hearts(grid, frame),
        Bubble::Zzz(frame) => return draw_zs(grid, frame),
        Bubble::Dots(n) => {
            let dots: String = (0..3).map(|i| if i < n { "wk" } else { "ww" }).collect();
            vec![format!("k{dots}wk")]
        }
        Bubble::Bang => BANG.iter().map(|s| s.to_string()).collect(),
    };
    grid.blit(&BUBBLE_FRAME, BUBBLE_X, 0);
    let first_row = if content.len() == 1 { 3 } else { 1 };
    let rows: Vec<&str> = content.iter().map(String::as_str).collect();
    grid.blit(&rows, BUBBLE_X, first_row);
}

pub fn render(look: &Look, coat: Coat) -> Frame {
    let mut grid = Grid::new(coat.animal());
    match look.pose {
        Pose::Walk(phase) => draw_standing(&mut grid, false, WALK_STRIDES[phase as usize % 4]),
        Pose::Stand => draw_standing(&mut grid, false, [0; 4]),
        Pose::Blink => draw_standing(&mut grid, true, [0; 4]),
        Pose::Sleep => draw_sleeping(&mut grid),
        Pose::Dangle { swing, kick } => hanging::draw_hanging(&mut grid, swing, kick),
        Pose::Fall => draw_standing(&mut grid, false, FALL_STRIDES),
        Pose::Cling(phase) => draw_clinging(&mut grid, WALK_STRIDES[phase as usize % 4]),
        Pose::Ceiling(phase) => draw_upside_down(&mut grid, WALK_STRIDES[phase as usize % 4]),
        Pose::Eat(bite) => play::draw_eating(&mut grid, bite),
        Pose::Swat(phase) => play::draw_swatting(&mut grid, phase),
        Pose::Hack(phase) => hairball::draw_hacking(&mut grid, phase),
        Pose::Squat(phase) => pig::draw_squatting(&mut grid, phase),
    }
    draw_bubble(&mut grid, look.bubble);

    let mut pixels = Vec::with_capacity(CANVAS_W * CANVAS_H);
    for y in 0..CANVAS_H {
        for x in 0..CANVAS_W {
            let src_x = match look.facing {
                Facing::Right => x,
                Facing::Left => CANVAS_W - 1 - x,
            };
            pixels.push(coat.color(grid.cells[y * CANVAS_W + src_x]));
        }
    }
    Frame {
        width: CANVAS_W,
        height: CANVAS_H,
        pixels,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(look: &Look) -> Frame {
        super::render(look, Coat::Ginger)
    }

    fn color(c: char) -> Option<Rgb> {
        Coat::Ginger.color(c)
    }

    fn look(pose: Pose, bubble: Bubble, facing: Facing) -> Look {
        Look {
            pose,
            bubble,
            facing,
        }
    }

    #[test]
    fn art_rows_fit_canvas_and_use_known_colors() {
        let all = HEAD_TOP
            .iter()
            .chain(&EYES_OPEN)
            .chain(&EYES_CLOSED)
            .chain(&BODY)
            .chain(&BUBBLE_FRAME)
            .chain(&BANG)
            .chain(&Z);
        for row in all {
            assert!(row.len() <= CANVAS_W, "row too wide: {row}");
            for coat in Coat::ALL {
                assert!(
                    row.chars().all(|c| c == '.' || coat.color(c).is_some()),
                    "unknown color in {row} for {coat:?}"
                );
            }
        }
    }

    #[test]
    fn mouth_is_on_the_walking_cat() {
        let (x, y) = MOUTH;
        for phase in 0..4 {
            let frame = render(&look(Pose::Walk(phase), Bubble::None, Facing::Right));
            assert!(frame.get(x, y).is_some());
            assert!(frame.get(x + 1, y).is_none(), "the front of the jaw");
        }
    }

    #[test]
    fn frame_has_canvas_size() {
        let frame = render(&look(Pose::Stand, Bubble::None, Facing::Right));
        assert_eq!(frame.pixels.len(), CANVAS_W * CANVAS_H);
    }

    #[test]
    fn left_facing_is_mirror_of_right() {
        let right = render(&look(Pose::Walk(0), Bubble::Bang, Facing::Right));
        let left = render(&look(Pose::Walk(0), Bubble::Bang, Facing::Left));
        for y in 0..CANVAS_H {
            for x in 0..CANVAS_W {
                assert_eq!(right.get(x, y), left.get(CANVAS_W - 1 - x, y));
            }
        }
    }

    #[test]
    fn walk_phases_differ() {
        let a = render(&look(Pose::Walk(0), Bubble::None, Facing::Right));
        let b = render(&look(Pose::Walk(2), Bubble::None, Facing::Right));
        assert_ne!(a, b);
    }

    #[test]
    fn bubble_area_is_empty_without_bubble() {
        let frame = render(&look(Pose::Stand, Bubble::None, Facing::Right));
        assert!((0..BUBBLE_H).all(|y| (0..CANVAS_W).all(|x| frame.get(x, y).is_none())));
    }

    #[test]
    fn dots_bubble_shows_requested_dot_count() {
        let dark = color('k');
        let count = |n| {
            let f = render(&look(Pose::Stand, Bubble::Dots(n), Facing::Right));
            (BUBBLE_X + 1..BUBBLE_X + 8)
                .filter(|&x| f.get(x, 3) == dark)
                .count()
        };
        assert_eq!(count(1), 1);
        assert_eq!(count(3), 3);
    }

    fn dangle(swing: i8, kick: Kick) -> Frame {
        render(&look(
            Pose::Dangle { swing, kick },
            Bubble::None,
            Facing::Right,
        ))
    }

    fn lowest_row_left_edge(frame: &Frame) -> (usize, usize) {
        let y = (0..CANVAS_H)
            .rev()
            .find(|&y| (0..CANVAS_W).any(|x| frame.get(x, y).is_some()))
            .unwrap();
        (
            y,
            (0..CANVAS_W).find(|&x| frame.get(x, y).is_some()).unwrap(),
        )
    }

    #[test]
    fn hanging_cat_is_held_at_the_scruff() {
        let frame = dangle(0, Kick::None);
        let (x, y) = SCRUFF;
        assert!(frame.get(x, y).is_some() || frame.get(x - 1, y).is_some());
        assert!((0..y).all(|row| (0..CANVAS_W).all(|col| frame.get(col, row).is_none())));
    }

    #[test]
    fn swing_moves_the_feet_but_not_the_scruff() {
        let still = dangle(0, Kick::None);
        let swung = dangle(3, Kick::None);
        let (y, x) = lowest_row_left_edge(&still);
        assert_eq!(lowest_row_left_edge(&swung), (y, x + 3));
        let (sx, sy) = SCRUFF;
        assert_eq!(still.get(sx, sy), swung.get(sx, sy));
    }

    #[test]
    fn kicking_changes_the_legs() {
        assert_ne!(dangle(0, Kick::Left), dangle(0, Kick::None));
        assert_ne!(dangle(0, Kick::Left), dangle(0, Kick::Right));
    }

    #[test]
    fn clinging_cat_touches_the_wall_and_fits_the_canvas() {
        let right = render(&look(Pose::Cling(1), Bubble::None, Facing::Right));
        let touches = |frame: &Frame, x: usize| (0..CANVAS_H).any(|y| frame.get(x, y).is_some());
        assert!(touches(&right, CANVAS_W - 1), "paws reach the right edge");
        let left = render(&look(Pose::Cling(1), Bubble::None, Facing::Left));
        assert!(touches(&left, 0), "mirrored: paws reach the left edge");
        assert!(touches(&right, CANVAS_W - CAT_H), "the whole body is drawn");
    }

    #[test]
    fn clinging_paws_move_with_the_phase() {
        let a = render(&look(Pose::Cling(0), Bubble::None, Facing::Right));
        let b = render(&look(Pose::Cling(2), Bubble::None, Facing::Right));
        assert_ne!(a, b);
    }

    #[test]
    fn upside_down_cat_hangs_from_the_top_edge() {
        let frame = render(&look(Pose::Ceiling(1), Bubble::None, Facing::Right));
        assert!(
            (0..CANVAS_W).any(|x| frame.get(x, 0).is_some()),
            "paws on the top row"
        );
        // The flipped cat fills the top CAT_H rows; nothing hangs below them.
        assert!((CAT_H..CANVAS_H).all(|y| (0..CANVAS_W).all(|x| frame.get(x, y).is_none())));
    }

    #[test]
    fn hearts_rise_above_the_cat_and_move() {
        let red = color('r');
        let hearts_at = |frame: u8| {
            let f = render(&look(Pose::Blink, Bubble::Hearts(frame), Facing::Right));
            let mut cells = Vec::new();
            for y in 0..CANVAS_H {
                for x in 0..CANVAS_W {
                    if f.get(x, y) == red {
                        cells.push((x, y));
                    }
                }
            }
            cells
        };
        let now = hearts_at(5);
        assert!(!now.is_empty());
        assert!(
            now.iter().all(|&(_, y)| y < BUBBLE_H),
            "hearts stay above the cat"
        );
        assert_ne!(now, hearts_at(6), "hearts move between frames");
    }

    #[test]
    fn zs_float_up_above_the_sleeping_cat() {
        let asleep = |bubble| render(&look(Pose::Sleep, bubble, Facing::Right));
        let cat = asleep(Bubble::None).pixels;
        let lowest_z_row = |frame: u8| {
            let f = asleep(Bubble::Zzz(frame));
            let below = SLEEP_TOP * CANVAS_W;
            assert_eq!(f.pixels[below..], cat[below..], "the cat is untouched");
            (0..SLEEP_TOP)
                .rev()
                .find(|&y| (0..CANVAS_W).any(|x| f.get(x, y).is_some()))
                .unwrap()
        };
        assert_eq!(lowest_z_row(0), SLEEP_TOP - 1, "just above the head");
        assert_eq!(lowest_z_row(1), SLEEP_TOP - 2, "z's rise a row per frame");
    }

    /// The two eye pixels on the lower eye row, where closed eyes are drawn.
    fn eye_colors(coat: Coat, pose: Pose) -> [Rgb; 2] {
        let frame = super::render(&look(pose, Bubble::None, Facing::Right), coat);
        let eye_row = BUBBLE_H + HEAD_TOP.len() + 1;
        [15, 20].map(|x| frame.get(x, eye_row).unwrap())
    }

    #[test]
    fn coats_color_the_cat_differently() {
        let stand = look(Pose::Stand, Bubble::None, Facing::Right);
        let cats = Coat::ALL
            .into_iter()
            .filter(|coat| coat.animal() == Animal::Cat);
        let frames: Vec<_> = cats.map(|coat| super::render(&stand, coat)).collect();
        assert_ne!(frames[0], frames[1]);
        assert_ne!(frames[1], frames[2]);
        assert_ne!(frames[0], frames[2]);
        let shape = |frame: &Frame| frame.pixels.iter().map(Option::is_some).collect::<Vec<_>>();
        assert!(
            frames.iter().all(|f| shape(f) == shape(&frames[0])),
            "same silhouette"
        );
    }

    #[test]
    fn alfie_has_odd_eyes_and_willow_blue_ones() {
        let alfie = eye_colors(Coat::Alfie, Pose::Stand);
        assert_ne!(alfie[0], alfie[1]);
        let willow = eye_colors(Coat::Willow, Pose::Stand);
        assert_eq!(willow[0], willow[1]);
        assert!(willow[0][2] > willow[0][0], "blue eyes");
    }

    #[test]
    fn a_custom_coat_wears_its_own_palette() {
        let palette = Palette {
            fur: [10, 20, 30],
            stripe: [40, 50, 60],
            blaze: [70, 80, 90],
            muzzle: [100, 110, 120],
            legs: [130, 140, 150],
            tail: [160, 170, 180],
            near_eye: [0, 200, 0],
            far_eye: [200, 0, 200],
        };
        let custom = Coat::Custom(palette);
        assert_eq!(custom.color('o'), Some(palette.fur));
        assert_eq!(custom.color('t'), Some(palette.tail));
        assert_eq!(custom.color('k'), color('k'), "outline stays");
        assert_eq!(
            eye_colors(custom, Pose::Stand),
            [palette.near_eye, palette.far_eye]
        );
    }

    #[test]
    fn closed_eyes_are_outlined_whatever_the_coat() {
        for coat in Coat::ALL
            .into_iter()
            .filter(|coat| coat.animal() == Animal::Cat)
        {
            let eyes = eye_colors(coat, Pose::Blink);
            assert!(eyes.iter().all(|&eye| Some(eye) == color('k')), "{coat:?}");
        }
    }

    #[test]
    fn bubble_and_hearts_keep_their_colors() {
        let bang = look(Pose::Stand, Bubble::Bang, Facing::Right);
        let hearts = look(Pose::Blink, Bubble::Hearts(5), Facing::Right);
        let zs = look(Pose::Sleep, Bubble::Zzz(5), Facing::Right);
        for shown in [bang, hearts, zs] {
            let ginger = super::render(&shown, Coat::Ginger);
            for coat in Coat::ALL {
                let other = super::render(&shown, coat);
                let above = |f: &Frame| f.pixels[..BUBBLE_H * CANVAS_W].to_vec();
                assert_eq!(above(&ginger), above(&other), "{coat:?}");
            }
        }
    }
}
