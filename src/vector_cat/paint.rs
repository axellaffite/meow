//! Drawing helpers over Core Graphics: shapes kept as plain segments (so the
//! same one can be filled, stroked or clipped to), colors, and an offscreen
//! bitmap to render into.

use std::ffi::c_void;

use objc2_core_foundation::{CFRetained, CGAffineTransform, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGBitmapContextCreateImage, CGBitmapContextGetBytesPerRow,
    CGBitmapContextGetData, CGColor, CGColorSpace, CGContext, CGImage, CGImageAlphaInfo, CGLineCap,
    CGLineJoin, CGPath, kCGColorSpaceSRGB,
};

use crate::sprites::Rgb;

/// A point in canvas units (sprite pixels), y down.
pub type P = (f64, f64);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    r: f64,
    g: f64,
    b: f64,
    a: f64,
}

impl Color {
    pub fn rgb(rgb: Rgb) -> Self {
        let channel = |v: u8| f64::from(v) / 255.0;
        Self {
            r: channel(rgb[0]),
            g: channel(rgb[1]),
            b: channel(rgb[2]),
            a: 1.0,
        }
    }

    /// `t` of the way to `other`.
    pub fn mix(self, other: Color, t: f64) -> Self {
        let lerp = |a: f64, b: f64| a + (b - a) * t;
        Self {
            r: lerp(self.r, other.r),
            g: lerp(self.g, other.g),
            b: lerp(self.b, other.b),
            a: lerp(self.a, other.a),
        }
    }

    pub fn lighter(self, t: f64) -> Self {
        self.mix(Color::WHITE.alpha(self.a), t)
    }

    pub fn darker(self, t: f64) -> Self {
        self.mix(Color::BLACK.alpha(self.a), t)
    }

    pub fn alpha(self, a: f64) -> Self {
        Self { a, ..self }
    }

    /// Perceived brightness, 0 to 1.
    pub fn luma(self) -> f64 {
        0.299 * self.r + 0.587 * self.g + 0.114 * self.b
    }

    pub const WHITE: Color = Color {
        r: 1.0,
        g: 1.0,
        b: 1.0,
        a: 1.0,
    };
    pub const BLACK: Color = Color {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    };

    fn cg(self) -> CFRetained<CGColor> {
        CGColor::new_srgb(self.r, self.g, self.b, self.a)
    }
}

#[derive(Clone, Copy, Debug)]
enum Seg {
    Move(P),
    Line(P),
    Quad(P, P),
    Curve(P, P, P),
    Close,
    Ellipse(P, f64, f64),
}

/// An outline, open or closed, made of lines, curves and ellipses.
#[derive(Clone, Debug, Default)]
pub struct Shape(Vec<Seg>);

impl Shape {
    pub fn at(p: P) -> Self {
        Self(vec![Seg::Move(p)])
    }

    pub fn ellipse(center: P, rx: f64, ry: f64) -> Self {
        Self(vec![Seg::Ellipse(center, rx, ry)])
    }

    pub fn circle(center: P, r: f64) -> Self {
        Self::ellipse(center, r, r)
    }

    /// A rectangle from `(x0, y0)` to `(x1, y1)` with corners rounded by `r`.
    pub fn rounded(x0: f64, y0: f64, x1: f64, y1: f64, r: f64) -> Self {
        // A quarter circle is a cubic with handles this far along its tangents.
        let k = r * 0.552;
        Self::at((x0 + r, y0))
            .line((x1 - r, y0))
            .curve((x1 - r + k, y0), (x1, y0 + r - k), (x1, y0 + r))
            .line((x1, y1 - r))
            .curve((x1, y1 - r + k), (x1 - r + k, y1), (x1 - r, y1))
            .line((x0 + r, y1))
            .curve((x0 + r - k, y1), (x0, y1 - r + k), (x0, y1 - r))
            .line((x0, y0 + r))
            .curve((x0, y0 + r - k), (x0 + r - k, y0), (x0 + r, y0))
            .close()
    }

    pub fn line(mut self, p: P) -> Self {
        self.0.push(Seg::Line(p));
        self
    }

    pub fn quad(mut self, control: P, p: P) -> Self {
        self.0.push(Seg::Quad(control, p));
        self
    }

    pub fn curve(mut self, c1: P, c2: P, p: P) -> Self {
        self.0.push(Seg::Curve(c1, c2, p));
        self
    }

    pub fn close(mut self) -> Self {
        self.0.push(Seg::Close);
        self
    }

    /// This shape and `other`, filled together.
    pub fn and(mut self, other: Shape) -> Self {
        self.0.extend(other.0);
        self
    }

    fn trace(&self, ctx: &CGContext) {
        let c = Some(ctx);
        CGContext::begin_path(c);
        for seg in &self.0 {
            match *seg {
                Seg::Move((x, y)) => CGContext::move_to_point(c, x, y),
                Seg::Line((x, y)) => CGContext::add_line_to_point(c, x, y),
                Seg::Quad((cx, cy), (x, y)) => CGContext::add_quad_curve_to_point(c, cx, cy, x, y),
                Seg::Curve((ax, ay), (bx, by), (x, y)) => {
                    CGContext::add_curve_to_point(c, ax, ay, bx, by, x, y)
                }
                Seg::Close => CGContext::close_path(c),
                Seg::Ellipse((x, y), rx, ry) => CGContext::add_ellipse_in_rect(
                    c,
                    CGRect::new(
                        CGPoint::new(x - rx, y - ry),
                        CGSize::new(2.0 * rx, 2.0 * ry),
                    ),
                ),
            }
        }
    }
}

/// Paints shapes on a context set up in canvas units.
pub struct Pen<'a> {
    ctx: &'a CGContext,
}

impl<'a> Pen<'a> {
    pub fn new(ctx: &'a CGContext) -> Self {
        let c = Some(ctx);
        CGContext::set_line_cap(c, CGLineCap::Round);
        CGContext::set_line_join(c, CGLineJoin::Round);
        Self { ctx }
    }

    pub fn fill(&self, shape: &Shape, color: Color) {
        let c = Some(self.ctx);
        shape.trace(self.ctx);
        CGContext::set_fill_color_with_color(c, Some(&color.cg()));
        CGContext::fill_path(c);
    }

    /// Fills a Core Graphics path, such as a glyph's outline.
    pub fn fill_path(&self, path: &CGPath, color: Color) {
        let c = Some(self.ctx);
        CGContext::begin_path(c);
        CGContext::add_path(c, Some(path));
        CGContext::set_fill_color_with_color(c, Some(&color.cg()));
        CGContext::fill_path(c);
    }

    pub fn stroke(&self, shape: &Shape, color: Color, width: f64) {
        let c = Some(self.ctx);
        shape.trace(self.ctx);
        CGContext::set_stroke_color_with_color(c, Some(&color.cg()));
        CGContext::set_line_width(c, width);
        CGContext::stroke_path(c);
    }

    /// Runs `draw` with painting limited to `shape`.
    pub fn clipped(&self, shape: &Shape, draw: impl FnOnce(&Pen)) {
        let c = Some(self.ctx);
        CGContext::save_g_state(c);
        shape.trace(self.ctx);
        CGContext::clip(c);
        draw(self);
        CGContext::restore_g_state(c);
    }

    /// Runs `draw` with `transform` applied on top of the current one.
    pub fn transformed(&self, transform: CGAffineTransform, draw: impl FnOnce(&Pen)) {
        let c = Some(self.ctx);
        CGContext::save_g_state(c);
        CGContext::concat_ctm(c, transform);
        draw(self);
        CGContext::restore_g_state(c);
    }
}

pub fn translate(dx: f64, dy: f64) -> CGAffineTransform {
    CGAffineTransform {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        tx: dx,
        ty: dy,
    }
}

/// A turn by `angle` radians (clockwise on the y-down canvas) about `center`.
pub fn rotate_about(center: P, angle: f64) -> CGAffineTransform {
    let (sin, cos) = angle.sin_cos();
    let (x, y) = center;
    CGAffineTransform {
        a: cos,
        b: sin,
        c: -sin,
        d: cos,
        tx: x - cos * x + sin * y,
        ty: y - sin * x - cos * y,
    }
}

/// Grown by `factor` about `center`.
pub fn scale_about(center: P, factor: f64) -> CGAffineTransform {
    let (x, y) = center;
    CGAffineTransform {
        a: factor,
        b: 0.0,
        c: 0.0,
        d: factor,
        tx: x - factor * x,
        ty: y - factor * y,
    }
}

/// Left and right swapped across a canvas `width` wide.
pub fn mirror(width: f64) -> CGAffineTransform {
    CGAffineTransform {
        a: -1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        tx: width,
        ty: 0.0,
    }
}

fn srgb() -> Option<CFRetained<CGColorSpace>> {
    // SAFETY: a constant of Core Graphics, valid for the life of the process.
    CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB }))
}

/// An offscreen RGBA image to draw in.
pub struct Bitmap {
    ctx: CFRetained<CGContext>,
    pub width: usize,
    pub height: usize,
}

impl Bitmap {
    /// A transparent bitmap `width` by `height` pixels.
    pub fn new(width: usize, height: usize) -> Option<Self> {
        let space = srgb()?;
        // SAFETY: a null buffer lets Core Graphics allocate and own the pixels;
        // 8-bit premultiplied RGBA is a supported bitmap format.
        let ctx = unsafe {
            CGBitmapContextCreate(
                std::ptr::null_mut::<c_void>(),
                width,
                height,
                8,
                0,
                Some(&space),
                CGImageAlphaInfo::PremultipliedLast.0,
            )
        }?;
        Some(Self { ctx, width, height })
    }

    /// Runs `draw` on a context with its origin at the top left corner, y
    /// down, and `scale` pixels to a unit.
    pub fn draw(&self, scale: f64, draw: impl FnOnce(&CGContext)) {
        let c = Some(&*self.ctx);
        CGContext::save_g_state(c);
        CGContext::translate_ctm(c, 0.0, self.height as f64);
        CGContext::scale_ctm(c, scale, -scale);
        draw(&self.ctx);
        CGContext::restore_g_state(c);
    }

    /// The pixels, top row first, as straight (not premultiplied) RGBA.
    pub fn rgba(&self) -> Vec<u8> {
        let c = Some(&*self.ctx);
        let data = CGBitmapContextGetData(c) as *const u8;
        let stride = CGBitmapContextGetBytesPerRow(c);
        if data.is_null() {
            return vec![0; self.width * self.height * 4];
        }
        // SAFETY: the context owns `stride * height` bytes at `data` while it lives.
        let bytes = unsafe { std::slice::from_raw_parts(data, stride * self.height) };
        let mut out = Vec::with_capacity(self.width * self.height * 4);
        for row in bytes.chunks(stride) {
            for px in row[..self.width * 4].chunks(4) {
                let a = px[3];
                let straight = |v: u8| match a {
                    0 => 0,
                    _ => ((u32::from(v) * 255 + u32::from(a) / 2) / u32::from(a)).min(255) as u8,
                };
                out.extend([straight(px[0]), straight(px[1]), straight(px[2]), a]);
            }
        }
        out
    }

    pub fn image(&self) -> Option<CFRetained<CGImage>> {
        CGBitmapContextCreateImage(Some(&self.ctx))
    }
}
