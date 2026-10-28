//! The Metal renderer: draws a creature's [`Frame`] into a texture, lit with
//! a soft stylized ramp over each material's base colour (see
//! `shaders/model.metal`), over a contact shadow, transparent elsewhere.
//!
//! Shaders (`shaders/*.metal`) are compiled at runtime with
//! `newLibraryWithSource`, so building needs no Xcode; [`Pending`] does it
//! on a worker thread, so the main thread never waits.
//!
//! How a frame is drawn:
//! 1. the main pass into a linear RGBA16Float target with MSAA (2x on
//!    Battery, 4x otherwise) and a depth buffer: the shadow, then the meshes
//!    (skinned on the GPU from `frame.skin`). Every surface is tonemapped
//!    (Khronos PBR Neutral) as it is shaded; MSAA resolves in linear;
//! 2. the encode pass into the target (`PIXEL_FORMAT`, BGRA8Unorm): sRGB
//!    of the straight colour, times alpha, so empty pixels stay (0, 0, 0, 0).
//!
//! The target covers the creature's window: its size in pixels over
//! [`RenderSettings::scale`] gives the window in points, where
//! `frame.model` places the creature.

mod hit;
mod pacer;
mod pending;
mod uniforms;

use std::ptr::NonNull;

use glam::{Mat4, Vec2, Vec3};
use objc2::Message;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_foundation::NSString;
use objc2_metal::{
    MTLBlendFactor, MTLBlitCommandEncoder, MTLBuffer, MTLClearColor, MTLCommandBuffer,
    MTLCommandEncoder, MTLCommandQueue, MTLCompareFunction, MTLCreateSystemDefaultDevice,
    MTLCullMode, MTLDepthStencilDescriptor, MTLDepthStencilState, MTLDevice, MTLGPUFamily,
    MTLIndexType, MTLLibrary, MTLLoadAction, MTLOrigin, MTLPixelFormat, MTLPrimitiveType,
    MTLRegion, MTLRenderCommandEncoder, MTLRenderPassDescriptor, MTLRenderPipelineDescriptor,
    MTLRenderPipelineState, MTLResourceOptions, MTLSize, MTLStorageMode, MTLStoreAction,
    MTLTexture, MTLTextureDescriptor, MTLTextureType, MTLTextureUsage, MTLWinding,
};

use crate::creature::asset::{CreatureAsset, Primitive};
use crate::creature::frame::Frame;

pub use hit::HitTest;
pub use pacer::{BATTERY_FPS, Canvas, Pacer};
pub use pending::{Pending, Share, low_power_device};
use uniforms::{DrawInfo, Globals, GpuVertex};
pub use uniforms::{PITCH_DEGREES, linear, view_matrix};

/// The shader sources, compiled together.
const SHADERS: &str = concat!(
    include_str!("shaders/common.metal"),
    include_str!("shaders/model.metal"),
    include_str!("shaders/post.metal"),
);

/// The linear image the creature is lit into, before the sRGB encode.
const LINEAR_FORMAT: MTLPixelFormat = MTLPixelFormat::RGBA16Float;
const DEPTH_FORMAT: MTLPixelFormat = MTLPixelFormat::Depth32Float;

/// The desktop's appearance: the rim light and the shadow follow it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Theme {
    #[default]
    Light,
    Dark,
}

/// How much the renderer may spend.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Quality {
    /// 2x MSAA, motion drawn at most [`BATTERY_FPS`].
    Battery,
    /// 4x MSAA, motion drawn every tick.
    #[default]
    Balanced,
}

impl Quality {
    /// MSAA samples asked for (the device may give fewer).
    pub fn samples(self) -> usize {
        match self {
            Quality::Battery => 2,
            Quality::Balanced => 4,
        }
    }

    /// How often a calm creature (asleep, only breathing) is redrawn: whole
    /// divisions of a 30 Hz tick.
    pub fn idle_fps(self) -> f32 {
        match self {
            Quality::Battery => 6.0,
            Quality::Balanced => 15.0,
        }
    }

    /// From a name, `battery` or `balanced`.
    pub fn from_name(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "battery" => Some(Quality::Battery),
            "balanced" => Some(Quality::Balanced),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderSettings {
    pub theme: Theme,
    pub quality: Quality,
    /// Seconds, for shaders that move by themselves.
    pub time: f32,
    /// Pixels per point: the window's backing scale (2 on Retina).
    pub scale: f32,
    /// How far the camera looks down, degrees.
    pub pitch: f32,
}

impl Default for RenderSettings {
    fn default() -> Self {
        Self {
            theme: Theme::Light,
            quality: Quality::Balanced,
            time: 0.0,
            scale: 1.0,
            pitch: PITCH_DEGREES,
        }
    }
}

type Texture = Retained<ProtocolObject<dyn MTLTexture>>;
type Buffer = Retained<ProtocolObject<dyn MTLBuffer>>;
type Pipeline = Retained<ProtocolObject<dyn MTLRenderPipelineState>>;
type DepthState = Retained<ProtocolObject<dyn MTLDepthStencilState>>;

/// One primitive on the GPU.
#[derive(Clone)]
struct Mesh {
    vertices: Buffer,
    indices: Buffer,
    count: usize,
    info: DrawInfo,
    double_sided: bool,
}

/// The pipelines of the main pass, for one sample count.
#[derive(Clone)]
struct MainPipelines {
    samples: usize,
    model: Pipeline,
    shadow: Pipeline,
}

/// The offscreen targets, for one size and sample count.
struct Targets {
    width: usize,
    height: usize,
    samples: usize,
    color: Option<Texture>,
    depth: Texture,
    resolved: Texture,
}

/// Draws one creature.
pub struct Renderer {
    device: Retained<ProtocolObject<dyn MTLDevice>>,
    library: Retained<ProtocolObject<dyn MTLLibrary>>,
    /// One set per sample count the qualities use, all compiled up front:
    /// a quality change (Low Power Mode) never compiles on the main thread.
    main: Vec<MainPipelines>,
    composite: Pipeline,
    depth_write: DepthState,
    depth_off: DepthState,
    meshes: Vec<Mesh>,
    skin_joints: usize,
    /// How far the creature's front is ahead of its origin at rest, meters:
    /// the camera pitches about it.
    front_z: f32,
    targets: Option<Targets>,
    offset: Vec2,
    /// The frame's skinning matrices for the GPU, reused each frame.
    skin: Vec<[[f32; 4]; 4]>,
}

impl Renderer {
    /// The pixel format targets are expected in (the window's `CAMetalLayer`).
    /// The renderer encodes sRGB itself in its last pass.
    pub const PIXEL_FORMAT: MTLPixelFormat = MTLPixelFormat::BGRA8Unorm;

    /// Compiles the shaders and uploads `asset`'s meshes to `device`. Takes
    /// a few milliseconds: on the main thread, go through [`Pending`].
    pub fn new(
        device: &ProtocolObject<dyn MTLDevice>,
        asset: &CreatureAsset,
    ) -> Result<Self, String> {
        let library = device
            .newLibraryWithSource_options_error(&NSString::from_str(SHADERS), None)
            .map_err(|e| format!("shaders: {}", e.localizedDescription()))?;
        let composite = make_pipeline(
            device,
            &library,
            ("composite_vertex", "composite_fragment"),
            Self::PIXEL_FORMAT,
            1,
            Blend::Replace,
            false,
        )?;
        let depth_state = |compare: MTLCompareFunction, write: bool| {
            let descriptor = MTLDepthStencilDescriptor::new();
            descriptor.setDepthCompareFunction(compare);
            descriptor.setDepthWriteEnabled(write);
            device
                .newDepthStencilStateWithDescriptor(&descriptor)
                .ok_or("could not make a depth state")
        };
        let meshes = asset
            .primitives
            .iter()
            .filter_map(|primitive| upload(device, primitive).transpose())
            .collect::<Result<Vec<_>, _>>()?;
        let mut renderer = Self {
            device: device.retain(),
            library,
            main: Vec::new(),
            composite,
            depth_write: depth_state(MTLCompareFunction::LessEqual, true)?,
            depth_off: depth_state(MTLCompareFunction::Always, false)?,
            meshes,
            skin_joints: asset.skeleton.skin_joints.max(1),
            front_z: front_z(asset),
            targets: None,
            offset: Vec2::ZERO,
            skin: Vec::new(),
        };
        for quality in [Quality::Battery, Quality::Balanced] {
            let samples = renderer.supported_samples(quality.samples());
            renderer.ensure_pipelines(samples)?;
        }
        Ok(renderer)
    }

    pub fn device(&self) -> &ProtocolObject<dyn MTLDevice> {
        &self.device
    }

    /// Another renderer for the same creature on the same device, sharing
    /// the compiled shaders, pipelines and uploaded meshes (no compile), with
    /// its own targets: one per window that shows it.
    pub fn share(&self) -> Renderer {
        Renderer {
            device: self.device.clone(),
            library: self.library.clone(),
            main: self.main.clone(),
            composite: self.composite.clone(),
            depth_write: self.depth_write.clone(),
            depth_off: self.depth_off.clone(),
            meshes: self.meshes.clone(),
            skin_joints: self.skin_joints,
            front_z: self.front_z,
            targets: None,
            offset: Vec2::ZERO,
            skin: Vec::new(),
        }
    }

    /// Draws the creature `offset` points (y up) off the window's corner,
    /// as a window kept whole on a screen does.
    pub fn set_offset(&mut self, offset: Vec2) {
        self.offset = offset;
    }

    /// Creature space to window points as drawn (the offset and the camera
    /// included): where [`HitTest`] looks for the pointer.
    pub fn to_window(&self, frame: &Frame, settings: &RenderSettings) -> Mat4 {
        let (model, view) = uniforms::camera(frame, self.offset, self.front_z, settings.pitch);
        view * model
    }

    /// Encodes `frame` into `command_buffer`, drawn over `target` (every
    /// pixel written: transparent where nothing is).
    pub fn draw(
        &mut self,
        command_buffer: &ProtocolObject<dyn MTLCommandBuffer>,
        target: &ProtocolObject<dyn MTLTexture>,
        frame: &Frame,
        settings: &RenderSettings,
    ) -> Result<(), String> {
        let (width, height) = (target.width(), target.height());
        if width == 0 || height == 0 {
            return Err(format!("cannot draw into {width} x {height} pixels"));
        }
        let samples = self.supported_samples(settings.quality.samples());
        self.ensure_pipelines(samples)?;
        self.ensure_targets(width, height, samples)?;

        let pixels = Vec2::new(width as f32, height as f32);
        let globals = uniforms::globals(frame, settings, pixels, self.offset, self.front_z);
        let mirrored = Mat4::from_cols_array_2d(&globals.model).determinant() < 0.0;
        let winding = if mirrored {
            MTLWinding::Clockwise
        } else {
            MTLWinding::CounterClockwise
        };
        // The skin padded with identities to the skin's size, into the
        // buffer kept from the last frame.
        self.skin.clear();
        self.skin
            .extend(frame.skin.iter().map(|m| m.to_cols_array_2d()));
        if self.skin.len() < self.skin_joints {
            self.skin
                .resize(self.skin_joints, Mat4::IDENTITY.to_cols_array_2d());
        }
        let skin = &self.skin;
        let skin_buffer = if std::mem::size_of_val(skin.as_slice()) > 4096 {
            Some(buffer_with(&self.device, skin)?)
        } else {
            None
        };

        let main = self.main.iter().find(|m| m.samples == samples);
        let (Some(targets), Some(main)) = (&self.targets, main) else {
            return Err("the renderer has no targets".into());
        };

        // 1. The main pass.
        let pass = MTLRenderPassDescriptor::new();
        // SAFETY: attachment 0 always exists.
        let attachment = unsafe { pass.colorAttachments().objectAtIndexedSubscript(0) };
        match &targets.color {
            Some(color) => {
                attachment.setTexture(Some(color));
                attachment.setResolveTexture(Some(&targets.resolved));
                attachment.setStoreAction(MTLStoreAction::MultisampleResolve);
            }
            None => {
                attachment.setTexture(Some(&targets.resolved));
                attachment.setStoreAction(MTLStoreAction::Store);
            }
        }
        attachment.setLoadAction(MTLLoadAction::Clear);
        attachment.setClearColor(TRANSPARENT);
        let depth = pass.depthAttachment();
        depth.setTexture(Some(&targets.depth));
        depth.setLoadAction(MTLLoadAction::Clear);
        depth.setStoreAction(MTLStoreAction::DontCare);
        depth.setClearDepth(1.0);
        let encoder = command_buffer
            .renderCommandEncoderWithDescriptor(&pass)
            .ok_or("could not start the main pass")?;
        set_globals(&encoder, &globals);
        set_skin(&encoder, skin, skin_buffer.as_deref());

        // The contact shadow, under everything.
        if frame.shadow.is_some() {
            encoder.setDepthStencilState(Some(&self.depth_off));
            encoder.setCullMode(MTLCullMode::None);
            encoder.setRenderPipelineState(&main.shadow);
            // SAFETY: a 4 vertex strip that reads only the globals.
            unsafe {
                encoder.drawPrimitives_vertexStart_vertexCount(
                    MTLPrimitiveType::TriangleStrip,
                    0,
                    4,
                )
            };
        }

        // The meshes, opaque.
        encoder.setDepthStencilState(Some(&self.depth_write));
        encoder.setFrontFacingWinding(winding);
        encoder.setRenderPipelineState(&main.model);
        for mesh in &self.meshes {
            encoder.setCullMode(if mesh.double_sided {
                MTLCullMode::None
            } else {
                MTLCullMode::Back
            });
            draw_mesh(&encoder, mesh);
        }
        encoder.endEncoding();

        // 2. Encode into the target.
        let pass = MTLRenderPassDescriptor::new();
        // SAFETY: attachment 0 always exists.
        let attachment = unsafe { pass.colorAttachments().objectAtIndexedSubscript(0) };
        attachment.setTexture(Some(target));
        attachment.setLoadAction(MTLLoadAction::DontCare);
        attachment.setStoreAction(MTLStoreAction::Store);
        let encoder = command_buffer
            .renderCommandEncoderWithDescriptor(&pass)
            .ok_or("could not start the encode pass")?;
        encoder.setRenderPipelineState(&self.composite);
        // SAFETY: the resolved image outlives the encoder; one full-window triangle.
        unsafe {
            encoder.setFragmentTexture_atIndex(Some(&targets.resolved), 0);
            encoder.drawPrimitives_vertexStart_vertexCount(MTLPrimitiveType::Triangle, 0, 3);
        }
        encoder.endEncoding();
        Ok(())
    }

    fn supported_samples(&self, wanted: usize) -> usize {
        [wanted, 4, 2, 1]
            .into_iter()
            .filter(|&n| n <= wanted)
            .find(|&n| n == 1 || self.device.supportsTextureSampleCount(n))
            .unwrap_or(1)
    }

    fn ensure_pipelines(&mut self, samples: usize) -> Result<(), String> {
        if self.main.iter().any(|m| m.samples == samples) {
            return Ok(());
        }
        let (device, library) = (&*self.device, &*self.library);
        let main = |functions: (&str, &str), blend: Blend| {
            make_pipeline(
                device,
                library,
                functions,
                LINEAR_FORMAT,
                samples,
                blend,
                true,
            )
        };
        self.main.push(MainPipelines {
            samples,
            model: main(("model_vertex", "model_fragment"), Blend::Replace)?,
            shadow: main(("shadow_vertex", "shadow_fragment"), Blend::Premultiplied)?,
        });
        Ok(())
    }

    fn ensure_targets(
        &mut self,
        width: usize,
        height: usize,
        samples: usize,
    ) -> Result<(), String> {
        if self
            .targets
            .as_ref()
            .is_some_and(|t| (t.width, t.height, t.samples) == (width, height, samples))
        {
            return Ok(());
        }
        let device = &*self.device;
        // Apple GPUs keep transient MSAA and depth in tile memory.
        let transient = if device.supportsFamily(MTLGPUFamily::Apple2) {
            MTLStorageMode::Memoryless
        } else {
            MTLStorageMode::Private
        };
        let size = (width, height);
        let color = if samples > 1 {
            Some(make_texture(
                device,
                LINEAR_FORMAT,
                size,
                samples,
                MTLTextureUsage::RenderTarget,
                transient,
            )?)
        } else {
            None
        };
        self.targets = Some(Targets {
            width,
            height,
            samples,
            color,
            depth: make_texture(
                device,
                DEPTH_FORMAT,
                size,
                samples,
                MTLTextureUsage::RenderTarget,
                transient,
            )?,
            resolved: make_texture(
                device,
                LINEAR_FORMAT,
                size,
                1,
                MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead,
                MTLStorageMode::Private,
            )?,
        });
        Ok(())
    }
}

/// The front of the meshes at rest, meters ahead of the origin (+Z).
fn front_z(asset: &CreatureAsset) -> f32 {
    asset
        .primitives
        .iter()
        .flat_map(|p| {
            p.positions.iter().map(|&v| {
                if p.is_skinned() {
                    v.z
                } else {
                    p.world.transform_point3(v).z
                }
            })
        })
        .fold(0.0, f32::max)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Blend {
    Replace,
    Premultiplied,
}

fn make_pipeline(
    device: &ProtocolObject<dyn MTLDevice>,
    library: &ProtocolObject<dyn MTLLibrary>,
    (vertex, fragment): (&str, &str),
    format: MTLPixelFormat,
    samples: usize,
    blend: Blend,
    depth: bool,
) -> Result<Pipeline, String> {
    let function = |name: &str| {
        library
            .newFunctionWithName(&NSString::from_str(name))
            .ok_or_else(|| format!("shaders: no function `{name}`"))
    };
    let descriptor = MTLRenderPipelineDescriptor::new();
    let (vertex_fn, fragment_fn) = (function(vertex)?, function(fragment)?);
    descriptor.setVertexFunction(Some(&vertex_fn));
    descriptor.setFragmentFunction(Some(&fragment_fn));
    descriptor.setRasterSampleCount(samples);
    descriptor.setLabel(Some(&NSString::from_str(fragment)));
    // SAFETY: attachment 0 always exists.
    let attachment = unsafe { descriptor.colorAttachments().objectAtIndexedSubscript(0) };
    attachment.setPixelFormat(format);
    match blend {
        Blend::Replace => attachment.setBlendingEnabled(false),
        Blend::Premultiplied => {
            attachment.setBlendingEnabled(true);
            attachment.setSourceRGBBlendFactor(MTLBlendFactor::One);
            attachment.setDestinationRGBBlendFactor(MTLBlendFactor::OneMinusSourceAlpha);
            attachment.setSourceAlphaBlendFactor(MTLBlendFactor::One);
            attachment.setDestinationAlphaBlendFactor(MTLBlendFactor::OneMinusSourceAlpha);
        }
    }
    if depth {
        descriptor.setDepthAttachmentPixelFormat(DEPTH_FORMAT);
    }
    device
        .newRenderPipelineStateWithDescriptor_error(&descriptor)
        .map_err(|e| format!("pipeline `{fragment}`: {}", e.localizedDescription()))
}

fn make_texture(
    device: &ProtocolObject<dyn MTLDevice>,
    format: MTLPixelFormat,
    (width, height): (usize, usize),
    samples: usize,
    usage: MTLTextureUsage,
    storage: MTLStorageMode,
) -> Result<Texture, String> {
    // SAFETY: a plain 2D texture descriptor, valid pixel format and size.
    let descriptor = unsafe {
        MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(
            format, width, height, false,
        )
    };
    if samples > 1 {
        descriptor.setTextureType(MTLTextureType::Type2DMultisample);
        // SAFETY: the count was checked with supportsTextureSampleCount.
        unsafe { descriptor.setSampleCount(samples) };
    }
    descriptor.setUsage(usage);
    descriptor.setStorageMode(storage);
    device
        .newTextureWithDescriptor(&descriptor)
        .ok_or_else(|| format!("could not make a {width} x {height} texture"))
}

fn buffer_with<T: Copy>(
    device: &ProtocolObject<dyn MTLDevice>,
    data: &[T],
) -> Result<Buffer, String> {
    let zeros = [0u8; 16];
    let (bytes, length) = if data.is_empty() {
        (NonNull::from(&zeros).cast(), zeros.len())
    } else {
        (
            NonNull::new(data.as_ptr().cast_mut().cast()).ok_or("no data")?,
            std::mem::size_of_val(data),
        )
    };
    // SAFETY: `bytes` points to `length` readable bytes, copied by Metal.
    unsafe {
        device.newBufferWithBytes_length_options(
            bytes,
            length,
            MTLResourceOptions::StorageModeShared,
        )
    }
    .ok_or_else(|| format!("could not make a {length} byte buffer"))
}

fn set_globals(encoder: &ProtocolObject<dyn MTLRenderCommandEncoder>, globals: &Globals) {
    let bytes = NonNull::from(globals).cast();
    let length = std::mem::size_of::<Globals>();
    // SAFETY: the globals are under 4 KB and copied by the encoder.
    unsafe {
        encoder.setVertexBytes_length_atIndex(bytes, length, 1);
        encoder.setFragmentBytes_length_atIndex(bytes, length, 0);
    }
}

fn set_skin(
    encoder: &ProtocolObject<dyn MTLRenderCommandEncoder>,
    skin: &[[[f32; 4]; 4]],
    buffer: Option<&ProtocolObject<dyn MTLBuffer>>,
) {
    // SAFETY: the skin holds every joint the meshes index (padded to the
    // skin's size, never empty); under 4 KB as bytes, else a buffer.
    unsafe {
        match buffer {
            Some(buffer) => encoder.setVertexBuffer_offset_atIndex(Some(buffer), 0, 2),
            None => encoder.setVertexBytes_length_atIndex(
                NonNull::from(&skin[0]).cast(),
                std::mem::size_of_val(skin),
                2,
            ),
        }
    }
}

fn draw_mesh(encoder: &ProtocolObject<dyn MTLRenderCommandEncoder>, mesh: &Mesh) {
    let info = NonNull::from(&mesh.info).cast();
    let length = std::mem::size_of::<DrawInfo>();
    // SAFETY: the buffers hold `count` indices into the mesh's vertices.
    unsafe {
        encoder.setVertexBuffer_offset_atIndex(Some(&mesh.vertices), 0, 0);
        encoder.setFragmentBytes_length_atIndex(info, length, 1);
        encoder.drawIndexedPrimitives_indexCount_indexType_indexBuffer_indexBufferOffset(
            MTLPrimitiveType::Triangle,
            mesh.count,
            MTLIndexType::UInt32,
            &mesh.indices,
            0,
        );
    }
}

/// One primitive's vertices and indices on the GPU; `None` for an empty one.
fn upload(
    device: &ProtocolObject<dyn MTLDevice>,
    primitive: &Primitive,
) -> Result<Option<Mesh>, String> {
    // Triangles past the vertices would read out of bounds: the asset checks
    // them, this keeps the GPU safe whatever built the primitive.
    let last = primitive.positions.len() as u32;
    let indices: Vec<u32> = primitive
        .indices
        .chunks_exact(3)
        .filter(|tri| tri.iter().all(|&i| i < last))
        .flatten()
        .copied()
        .collect();
    if indices.is_empty() {
        return Ok(None);
    }
    Ok(Some(Mesh {
        vertices: buffer_with(device, &gpu_vertices(primitive))?,
        count: indices.len(),
        indices: buffer_with(device, &indices)?,
        info: DrawInfo::new(&primitive.material, !primitive.colors.is_empty()),
        double_sided: primitive.material.double_sided,
    }))
}

/// The primitive's attributes interleaved; an unskinned one is placed by its
/// node's matrix here, so the shaders treat every vertex alike.
fn gpu_vertices(primitive: &Primitive) -> Vec<GpuVertex> {
    let n = primitive.positions.len();
    let normals = if primitive.normals.len() == n {
        primitive.normals.clone()
    } else {
        smooth_normals(&primitive.positions, &primitive.indices)
    };
    let skinned = primitive.is_skinned() && primitive.weights.len() == n;
    let world = primitive.world;
    let normal_world = glam::Mat3::from_mat4(world).inverse().transpose();
    (0..n)
        .map(|i| {
            let (mut position, mut normal) = (primitive.positions[i], normals[i]);
            if !skinned {
                position = world.transform_point3(position);
                normal = (normal_world * normal).normalize_or(Vec3::Z);
            }
            let (joints, weights) = if skinned {
                (
                    primitive.joints[i].map(f32::from),
                    primitive.weights[i].to_array(),
                )
            } else {
                ([0.0; 4], [0.0; 4])
            };
            GpuVertex {
                position: position.to_array(),
                normal: normal.to_array(),
                uv0: primitive.uv0.get(i).copied().unwrap_or_default().to_array(),
                color: primitive
                    .colors
                    .get(i)
                    .copied()
                    .unwrap_or(glam::Vec4::ONE)
                    .to_array(),
                joints,
                weights,
            }
        })
        .collect()
}

fn smooth_normals(positions: &[Vec3], indices: &[u32]) -> Vec<Vec3> {
    let mut normals = vec![Vec3::ZERO; positions.len()];
    for tri in indices.chunks_exact(3) {
        let [a, b, c] = [tri[0], tri[1], tri[2]].map(|i| i as usize);
        if a >= positions.len() || b >= positions.len() || c >= positions.len() {
            continue;
        }
        let face = (positions[b] - positions[a]).cross(positions[c] - positions[a]);
        for i in [a, b, c] {
            normals[i] += face;
        }
    }
    normals
        .into_iter()
        .map(|n| n.normalize_or(Vec3::Z))
        .collect()
}

const TRANSPARENT: MTLClearColor = MTLClearColor {
    red: 0.0,
    green: 0.0,
    blue: 0.0,
    alpha: 0.0,
};

/// Renders `frame` offscreen at `width` x `height` pixels: straight-alpha
/// RGBA, rows from the top, as a PNG stores them.
pub fn render_rgba(
    asset: &CreatureAsset,
    frame: &Frame,
    settings: &RenderSettings,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, String> {
    let offscreen = Offscreen::new(width, height)?;
    let mut renderer = Renderer::new(offscreen.device(), asset)?;
    Ok(offscreen.render(&mut renderer, frame, settings, true)?.rgba)
}

/// A reusable offscreen target, for sequences and timing: what the examples
/// and tests use.
pub struct Offscreen {
    device: Retained<ProtocolObject<dyn MTLDevice>>,
    queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
    target: Texture,
    width: u32,
    height: u32,
}

/// What one offscreen frame gave.
pub struct Rendered {
    /// Straight-alpha RGBA, rows from the top (empty without a readback).
    pub rgba: Vec<u8>,
    /// GPU time of the frame's passes, milliseconds.
    pub gpu_ms: f64,
}

impl Offscreen {
    pub fn new(width: u32, height: u32) -> Result<Self, String> {
        if width == 0 || height == 0 {
            return Err(format!("cannot render {width} x {height} pixels"));
        }
        let device = MTLCreateSystemDefaultDevice().ok_or("no Metal device")?;
        let target = readable_target(&device, width, height)?;
        let queue = device.newCommandQueue().ok_or("no command queue")?;
        Ok(Self {
            device,
            queue,
            target,
            width,
            height,
        })
    }

    pub fn device(&self) -> &ProtocolObject<dyn MTLDevice> {
        &self.device
    }

    /// Draws `frame` with `renderer` and, if `readback`, reads it back.
    pub fn render(
        &self,
        renderer: &mut Renderer,
        frame: &Frame,
        settings: &RenderSettings,
        readback: bool,
    ) -> Result<Rendered, String> {
        let command_buffer = self.queue.commandBuffer().ok_or("no command buffer")?;
        renderer.draw(&command_buffer, &self.target, frame, settings)?;
        if readback {
            let blit = command_buffer
                .blitCommandEncoder()
                .ok_or("could not start a blit pass")?;
            blit.synchronizeResource(ProtocolObject::from_ref(&*self.target));
            blit.endEncoding();
        }
        command_buffer.commit();
        command_buffer.waitUntilCompleted();
        if let Some(error) = command_buffer.error() {
            return Err(format!("render failed: {}", error.localizedDescription()));
        }
        let gpu_ms =
            (command_buffer.GPUEndTime() - command_buffer.GPUStartTime()).max(0.0) * 1000.0;
        let rgba = if readback {
            straight_rgba(&read_target(&self.target, self.width, self.height)?)
        } else {
            Vec::new()
        };
        Ok(Rendered { rgba, gpu_ms })
    }
}

fn readable_target(
    device: &ProtocolObject<dyn MTLDevice>,
    width: u32,
    height: u32,
) -> Result<Texture, String> {
    // SAFETY: a plain 2D texture descriptor, valid pixel format and size.
    let descriptor = unsafe {
        MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(
            Renderer::PIXEL_FORMAT,
            width as usize,
            height as usize,
            false,
        )
    };
    descriptor.setUsage(MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead);
    // Managed works on every Mac GPU; a blit synchronizes it back.
    descriptor.setStorageMode(MTLStorageMode::Managed);
    device
        .newTextureWithDescriptor(&descriptor)
        .ok_or_else(|| "could not make the target texture".to_string())
}

fn read_target(
    target: &ProtocolObject<dyn MTLTexture>,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, String> {
    let row = width as usize * 4;
    let mut bgra = vec![0u8; row * height as usize];
    let region = MTLRegion {
        origin: MTLOrigin { x: 0, y: 0, z: 0 },
        size: MTLSize {
            width: width as usize,
            height: height as usize,
            depth: 1,
        },
    };
    let bytes = NonNull::new(bgra.as_mut_ptr().cast()).ok_or("no pixel buffer")?;
    // SAFETY: `bgra` holds exactly `height` rows of `row` bytes, the region's size.
    unsafe { target.getBytes_bytesPerRow_fromRegion_mipmapLevel(bytes, row, region, 0) };
    Ok(bgra)
}

/// Premultiplied BGRA to straight RGBA, as PNG stores it.
fn straight_rgba(bgra: &[u8]) -> Vec<u8> {
    bgra.chunks_exact(4)
        .flat_map(|px| {
            let (b, g, r, a) = (px[0], px[1], px[2], px[3]);
            let unmultiply = |c: u8| match a {
                0 => 0,
                a => ((u32::from(c) * 255 + u32::from(a) / 2) / u32::from(a)).min(255) as u8,
            };
            [unmultiply(r), unmultiply(g), unmultiply(b), a]
        })
        .collect()
}

#[cfg(test)]
mod tests;
