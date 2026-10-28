//! What the shaders read, laid out exactly as `shaders/common.metal` declares
//! it: every field a multiple of 16 bytes, so `#[repr(C)]` matches Metal.

use glam::{Mat3, Mat4, Vec2, Vec3, Vec4};

use super::{RenderSettings, Theme};
use crate::creature::asset::Material;
use crate::creature::frame::Frame;

/// How far the camera looks down at the creature by default, degrees.
pub const PITCH_DEGREES: f32 = 6.0;

/// One vertex of a mesh, as `Vertex` in the shaders.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GpuVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv0: [f32; 2],
    pub color: [f32; 4],
    pub joints: [f32; 4],
    pub weights: [f32; 4],
}

type F4 = [f32; 4];
type M4 = [[f32; 4]; 4];

/// The frame's uniforms, shared by every pass.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Globals {
    pub model: M4,
    pub view: M4,
    pub proj: M4,
    /// Inverse transpose of `view * model`, for normals.
    pub normal_mat: M4,
    /// Target width and height in pixels, pixels per point, points per meter.
    pub viewport: F4,
    /// Time, theme (0 light, 1 dark), quality (0, 1), mirror sign.
    pub misc: F4,
    pub key_dir: F4,
    pub key_col: F4,
    pub back_dir: F4,
    pub back_col: F4,
    pub sky: F4,
    pub ground: F4,
    pub rim: F4,
    /// Shadow centre x, ground y (window points), half width (points), alpha.
    pub shadow: F4,
    /// Shadow half height (points), unused.
    pub shadow2: F4,
}

/// Per draw: one primitive's material.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DrawInfo {
    /// Linear RGBA.
    pub base_color: F4,
    /// Roughness, metallic, double sided (0 or 1), has vertex colours (0 or 1).
    pub params: F4,
    /// Linear RGB added to the lit colour, unused.
    pub emissive: F4,
}

impl DrawInfo {
    pub fn new(material: &Material, vertex_colors: bool) -> Self {
        let flag = |b: bool| f32::from(u8::from(b));
        Self {
            base_color: material.base_color.to_array(),
            params: [
                material.roughness,
                material.metallic,
                flag(material.double_sided),
                flag(vertex_colors),
            ],
            emissive: v4(material.emissive, 0.0),
        }
    }
}

fn v4(v: Vec3, w: f32) -> F4 {
    [v.x, v.y, v.z, w]
}

/// An sRGB colour (`0xRRGGBB`) as linear RGB.
pub fn linear(hex: u32) -> Vec3 {
    let channel = |c: u32| {
        let c = (c & 0xFF) as f32 / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    Vec3::new(channel(hex >> 16), channel(hex >> 8), channel(hex))
}

/// The camera: window points to view points, pitched down `pitch` degrees
/// about the ground line at `ground` (window y), `pivot_z` points in front
/// (the creature's front, which then stays on the ground line).
pub fn view_matrix(ground: f32, pivot_z: f32, pitch: f32) -> Mat4 {
    let pivot = Vec3::new(0.0, ground, pivot_z);
    Mat4::from_translation(pivot)
        * Mat4::from_rotation_x(pitch.to_radians())
        * Mat4::from_translation(-pivot)
}

/// Orthographic over the window (`size` points), depth 0 nearest.
pub fn projection(size: Vec2) -> Mat4 {
    let depth = 2.0 * size.max_element().max(1.0);
    // x: 0..w -> -1..1, y: 0..h -> -1..1, z: +depth (near) -> 0, -depth -> 1.
    Mat4::from_cols(
        Vec4::new(2.0 / size.x, 0.0, 0.0, 0.0),
        Vec4::new(0.0, 2.0 / size.y, 0.0, 0.0),
        Vec4::new(0.0, 0.0, -0.5 / depth, 0.0),
        Vec4::new(-1.0, -1.0, 0.5, 1.0),
    )
}

/// The model matrix as drawn (`frame.model` shifted `offset` points) and
/// the camera that looks at it: the ground line under the creature's
/// origin, pitched about its front, `front_z` meters ahead of the origin.
pub fn camera(frame: &Frame, offset: Vec2, front_z: f32, pitch: f32) -> (Mat4, Mat4) {
    let model = Mat4::from_translation(offset.extend(0.0)) * frame.model;
    let points_per_meter = Mat3::from_mat4(frame.model).y_axis.length().max(1e-3);
    let height = frame.shadow.map_or(0.0, |s| s.height.max(0.0));
    let ground = model.transform_point3(Vec3::new(0.0, -height, 0.0)).y;
    let view = view_matrix(ground, front_z * points_per_meter, pitch);
    (model, view)
}

/// Everything the shaders need for `frame` in a `pixels` sized target.
pub fn globals(
    frame: &Frame,
    settings: &RenderSettings,
    pixels: Vec2,
    offset: Vec2,
    front_z: f32,
) -> Globals {
    let scale = if settings.scale.is_finite() {
        settings.scale.max(0.1)
    } else {
        1.0
    };
    let window = pixels / scale;
    let (model, view) = camera(frame, offset, front_z, settings.pitch);
    let proj = projection(window);
    let model_view = Mat3::from_mat4(view * model);
    let normal_mat = if model_view.determinant().abs() > 1e-12 {
        Mat4::from_mat3(model_view.inverse().transpose())
    } else {
        Mat4::IDENTITY
    };
    let points_per_meter = Mat3::from_mat4(frame.model).y_axis.length().max(1e-3);
    let mirror = if Mat3::from_mat4(frame.model).determinant() < 0.0 {
        -1.0
    } else {
        1.0
    };
    let dark = settings.theme == Theme::Dark;
    let quality = match settings.quality {
        super::Quality::Battery => 0.0,
        super::Quality::Balanced => 1.0,
    };
    // In view space: the key up and to the viewer's left, the back light
    // behind on the right.
    let key_dir = Vec3::new(-0.45, 0.70, 0.55).normalize();
    let back_dir = Vec3::new(0.55, 0.50, -0.65).normalize();

    // It shrinks and fades as the creature rises.
    let (shadow, shadow2) = match frame.shadow {
        Some(s) => {
            let h = s.height.max(0.0);
            let shrink = 1.0 / (1.0 + h / 0.30);
            // Over a dark desktop a dark shadow barely shows: denser there.
            let alpha = if dark { 0.55 } else { 0.30 } * (-h / 0.40).exp();
            let centre = model.transform_point3(Vec3::new(s.center_x, -h, 0.0));
            let half_width = s.radius.max(0.0) * points_per_meter * shrink;
            (
                [centre.x, centre.y, half_width, alpha],
                [half_width * 0.22, 0.0, 0.0, 0.0],
            )
        }
        None => ([0.0; 4], [0.0; 4]),
    };

    Globals {
        model: model.to_cols_array_2d(),
        view: view.to_cols_array_2d(),
        proj: proj.to_cols_array_2d(),
        normal_mat: normal_mat.to_cols_array_2d(),
        viewport: [pixels.x, pixels.y, scale, points_per_meter],
        misc: [settings.time, f32::from(u8::from(dark)), quality, mirror],
        key_dir: v4(key_dir, 1.0),
        key_col: v4(linear(0xFFF1DE), 1.0),
        back_dir: v4(back_dir, if dark { 1.0 } else { 0.8 }),
        back_col: v4(linear(0xFFE2B8), 1.0),
        sky: v4(linear(0xCFE0FF), if dark { 0.25 } else { 0.35 }),
        ground: v4(linear(0xF2D6C4), 0.15),
        rim: v4(linear(0xFFF4E8), 1.0),
        shadow,
        shadow2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layouts_are_whole_float4s() {
        assert_eq!(std::mem::size_of::<GpuVertex>(), 80);
        assert_eq!(std::mem::size_of::<Globals>() % 16, 0);
        assert_eq!(std::mem::size_of::<DrawInfo>(), 48);
        // setVertexBytes takes at most 4 KB.
        assert!(std::mem::size_of::<Globals>() < 4096);
    }

    #[test]
    fn the_projection_covers_the_window() {
        let proj = projection(Vec2::new(180.0, 225.0));
        let corner = proj * Vec4::new(180.0, 225.0, 0.0, 1.0);
        assert!((corner.x - 1.0).abs() < 1e-5 && (corner.y - 1.0).abs() < 1e-5);
        let near = proj * Vec4::new(0.0, 0.0, 100.0, 1.0);
        let far = proj * Vec4::new(0.0, 0.0, -100.0, 1.0);
        assert!(near.z < far.z && near.z > 0.0 && far.z < 1.0);
    }

    #[test]
    fn the_pitch_keeps_the_ground_line() {
        let view = view_matrix(10.0, 12.0, PITCH_DEGREES);
        let toe = view * Vec4::new(50.0, 10.0, 12.0, 1.0);
        assert!((toe.y - 10.0).abs() < 1e-4);
        // Behind the front, the ground rises a little: never under the window.
        let heel = view * Vec4::new(50.0, 10.0, -8.0, 1.0);
        assert!(heel.y > 10.0);
        // The top tips toward the viewer.
        let top = view * Vec4::new(0.0, 160.0, 0.0, 1.0);
        assert!(top.z > 0.0);
    }

    #[test]
    fn srgb_colours_are_made_linear() {
        assert_eq!(linear(0xFFFFFF), Vec3::ONE);
        assert_eq!(linear(0x000000), Vec3::ZERO);
        let mid = linear(0x808080).x;
        assert!((mid - 0.2158).abs() < 1e-3, "{mid}");
    }
}
