//! A creature as Blender exports it: one GLB with its meshes, skeleton,
//! sockets, clips and custom properties (the contract is in docs/3d.md).
//!
//! Nothing is converted at load: positions, joints and clips stay in glTF
//! space, in meters, Y up, the creature facing +Z, its left toward +X.
//! Malformed input is an error, never a panic: every buffer range, and the
//! layout of every accessor read, is checked before it is read.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use glam::{Mat4, Quat, Vec2, Vec3, Vec4};
use gltf::accessor::{DataType, Dimensions};
use gltf::animation::Property;
use gltf::animation::util::ReadOutputs;
use gltf::mesh::Mode;
use serde_json::Value;

use super::pose::{Pose, Transform};

/// Nodes named so are sockets: empties parented to a bone.
pub const SOCKET_PREFIX: &str = "socket_";
/// Bones named so carry data, not geometry: non-deform bones whose
/// channels a clip uses as numbers (an expression, a mode).
pub const CTRL_PREFIX: &str = "ctrl_";

/// Everything the motion engine and the renderer need of the creature.
#[derive(Clone, Debug)]
pub struct CreatureAsset {
    pub primitives: Vec<Primitive>,
    pub skeleton: Skeleton,
    pub sockets: Vec<Socket>,
    pub clips: Vec<Clip>,
    /// The extras (Blender custom properties) of every named node that has
    /// some, by node name: the rig's, a bone's, a socket's, a mesh's.
    pub extras: BTreeMap<String, Value>,
    /// The scene's extras (the Blender scene's custom properties), or `Null`.
    pub scene_extras: Value,
}

/// A primitive's glTF material: its PBR factors, no textures.
#[derive(Clone, Debug, PartialEq)]
pub struct Material {
    /// Its name in Blender; `""` when the primitive has none.
    pub name: String,
    /// Linear RGBA, times `COLOR_0` when the mesh has one.
    pub base_color: Vec4,
    pub roughness: f32,
    pub metallic: f32,
    /// Linear RGB, added to the lit colour.
    pub emissive: Vec3,
    /// Both faces drawn (Blender's backface culling off).
    pub double_sided: bool,
}

impl Material {
    fn of(material: &gltf::Material<'_>) -> Self {
        let pbr = material.pbr_metallic_roughness();
        Self {
            name: material.name().unwrap_or("").to_string(),
            base_color: Vec4::from(pbr.base_color_factor()),
            roughness: pbr.roughness_factor(),
            metallic: pbr.metallic_factor(),
            emissive: Vec3::from(material.emissive_factor()),
            double_sided: material.double_sided(),
        }
    }
}

/// One drawable piece of a mesh: one material, triangles. Attributes the
/// export left out are empty; those present have one entry per vertex.
#[derive(Clone, Debug)]
pub struct Primitive {
    /// The name of the mesh node.
    pub mesh: String,
    pub material: Material,
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    /// `TEXCOORD_0`.
    pub uv0: Vec<Vec2>,
    /// `COLOR_0` as RGBA.
    pub colors: Vec<Vec4>,
    /// `JOINTS_0`: indices into the skeleton's first `skin_joints` joints.
    /// An unskinned mesh parented to a bone gets that bone, whole.
    pub joints: Vec<[u16; 4]>,
    /// `WEIGHTS_0`.
    pub weights: Vec<Vec4>,
    /// Triangle list; `0..n` when the export had no indices.
    pub indices: Vec<u32>,
    /// The mesh node's world matrix at rest. glTF ignores it for skinned
    /// primitives (their joints place them); it places unskinned ones.
    pub world: Mat4,
}

impl Primitive {
    pub fn is_skinned(&self) -> bool {
        !self.joints.is_empty()
    }
}

/// One bone. The skin's joints come first, in the skin's order; bones the
/// skin leaves out (non-deform `ctrl_*` bones, when the export drops them
/// from it) follow, found by walking the nodes under the armature.
#[derive(Clone, Debug, PartialEq)]
pub struct Joint {
    pub name: String,
    /// Index of the parent joint; `None` for a top bone (its parent is the
    /// armature node, whose placement is [`Skeleton::root`]).
    pub parent: Option<usize>,
    /// The local transform at rest (the bind pose).
    pub rest: Transform,
    /// The skin's inverse bind matrix; for a bone outside the skin, the
    /// inverse of its rest world matrix.
    pub inverse_bind: Mat4,
    /// The bone's custom properties, or `Null`.
    pub extras: Value,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Skeleton {
    pub joints: Vec<Joint>,
    /// How many of `joints` the skin has: `JOINTS_0` indexes these, and
    /// [`Pose::skin`] returns that many matrices.
    pub skin_joints: usize,
    /// The world matrix of the armature node the top bones hang from.
    pub root: Mat4,
    /// Joint indices with every parent before its children.
    order: Vec<usize>,
}

impl Skeleton {
    /// Checks the parent links (in range, no cycle) and orders the joints.
    pub fn new(joints: Vec<Joint>, skin_joints: usize, root: Mat4) -> Result<Self, String> {
        if skin_joints > joints.len() {
            return Err(format!(
                "skeleton: {skin_joints} skin joints but only {} joints",
                joints.len()
            ));
        }
        let mut order = Vec::with_capacity(joints.len());
        let mut state = vec![0u8; joints.len()]; // 0 new, 1 visiting, 2 placed
        for start in 0..joints.len() {
            let mut chain = Vec::new();
            let mut at = Some(start);
            while let Some(index) = at {
                match state[index] {
                    2 => break,
                    1 => {
                        return Err(format!(
                            "skeleton: `{}` is its own ancestor",
                            joints[index].name
                        ));
                    }
                    _ => {}
                }
                state[index] = 1;
                chain.push(index);
                at = match joints[index].parent {
                    Some(parent) if parent >= joints.len() => {
                        return Err(format!(
                            "skeleton: `{}` has parent {parent}, out of range",
                            joints[index].name
                        ));
                    }
                    parent => parent,
                };
            }
            for &index in chain.iter().rev() {
                state[index] = 2;
                order.push(index);
            }
        }
        Ok(Self {
            joints,
            skin_joints,
            root,
            order,
        })
    }

    /// The index of the joint named `name`.
    pub fn find(&self, name: &str) -> Option<usize> {
        self.joints.iter().position(|joint| joint.name == name)
    }

    /// Joint indices, every parent before its children.
    pub fn order(&self) -> &[usize] {
        &self.order
    }
}

/// An empty named `socket_*`, parented to a bone: where things attach.
#[derive(Clone, Debug, PartialEq)]
pub struct Socket {
    pub name: String,
    /// The joint it hangs from.
    pub joint: usize,
    /// Its transform relative to that joint.
    pub local: Transform,
    pub extras: Value,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interpolation {
    Linear,
    /// glTF `STEP` (Blender's CONSTANT): data channels that switch use it.
    Step,
    /// Values hold (in-tangent, value, out-tangent) triplets per key.
    CubicSpline,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelPath {
    Translation,
    Rotation,
    Scale,
}

/// A channel's key values (three per key for [`Interpolation::CubicSpline`]).
#[derive(Clone, Debug, PartialEq)]
pub enum Keys {
    Translation(Vec<Vec3>),
    Rotation(Vec<Quat>),
    Scale(Vec<Vec3>),
}

/// One animated property of one joint.
#[derive(Clone, Debug, PartialEq)]
pub struct Channel {
    pub joint: usize,
    pub interpolation: Interpolation,
    /// Key times in seconds, increasing.
    pub times: Vec<f32>,
    pub keys: Keys,
}

impl Channel {
    pub fn path(&self) -> ChannelPath {
        match self.keys {
            Keys::Translation(_) => ChannelPath::Translation,
            Keys::Rotation(_) => ChannelPath::Rotation,
            Keys::Scale(_) => ChannelPath::Scale,
        }
    }

    /// Sets this channel's property of `local` to its value at `time`
    /// (clamped to the keys' range).
    pub fn apply(&self, time: f32, local: &mut Transform) {
        let Some((i, j, w, dt)) = segment(&self.times, time) else {
            return;
        };
        let step = self.interpolation == Interpolation::Step;
        match &self.keys {
            Keys::Translation(v) => local.t = self.vec3(v, i, j, w, dt, step),
            Keys::Scale(v) => local.s = self.vec3(v, i, j, w, dt, step),
            Keys::Rotation(q) => {
                local.r = match self.interpolation {
                    Interpolation::Step => q[i],
                    Interpolation::Linear => q[i].slerp(q[j], w),
                    Interpolation::CubicSpline => {
                        let (p0, m0) = (q[3 * i + 1], q[3 * i + 2] * dt);
                        let (p1, m1) = (q[3 * j + 1], q[3 * j] * dt);
                        let h = hermite(w);
                        Quat::from_vec4(
                            Vec4::from(p0) * h[0]
                                + Vec4::from(m0) * h[1]
                                + Vec4::from(p1) * h[2]
                                + Vec4::from(m1) * h[3],
                        )
                    }
                }
                .normalize();
            }
        }
    }

    fn vec3(&self, v: &[Vec3], i: usize, j: usize, w: f32, dt: f32, step: bool) -> Vec3 {
        match self.interpolation {
            _ if step => v[i],
            Interpolation::CubicSpline => {
                let h = hermite(w);
                v[3 * i + 1] * h[0]
                    + v[3 * i + 2] * dt * h[1]
                    + v[3 * j + 1] * h[2]
                    + v[3 * j] * dt * h[3]
            }
            _ => v[i].lerp(v[j], w),
        }
    }
}

/// The keys around `time`: (before, after, weight, seconds between them).
fn segment(times: &[f32], time: f32) -> Option<(usize, usize, f32, f32)> {
    let last = times.len().checked_sub(1)?;
    if time <= times[0] {
        return Some((0, 0, 0.0, 0.0));
    }
    if time >= times[last] {
        return Some((last, last, 0.0, 0.0));
    }
    let j = times.partition_point(|&t| t <= time);
    let i = j - 1;
    let dt = times[j] - times[i];
    let w = if dt > 0.0 {
        (time - times[i]) / dt
    } else {
        0.0
    };
    Some((i, j, w, dt))
}

/// Cubic Hermite basis at `w`: weights of p0, m0, p1, m1.
fn hermite(w: f32) -> [f32; 4] {
    let (w2, w3) = (w * w, w * w * w);
    [
        2.0 * w3 - 3.0 * w2 + 1.0,
        w3 - 2.0 * w2 + w,
        -2.0 * w3 + 3.0 * w2,
        w3 - w2,
    ]
}

/// One animation (a Blender action), in place, starting at 0.
#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    pub name: String,
    /// The last key time over all channels, in seconds (a loop of N frames at
    /// 30 fps lasts N/30 s: its key N equals key 0).
    pub duration: f32,
    pub channels: Vec<Channel>,
}

impl Clip {
    /// Sets the joints this clip animates to their values at `time` (clamped
    /// to the clip); the other joints of `pose` are left as they are.
    pub fn sample(&self, time: f32, pose: &mut Pose) {
        for channel in &self.channels {
            if let Some(local) = pose.locals.get_mut(channel.joint) {
                channel.apply(time, local);
            }
        }
    }
}

impl CreatureAsset {
    /// Reads the GLB at `path`.
    pub fn load(path: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::from_glb(&bytes).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Reads a GLB whose buffers are all in its binary chunk.
    pub fn from_glb(bytes: &[u8]) -> Result<Self, String> {
        if !bytes.starts_with(b"glTF") {
            return Err("not a GLB file".to_string());
        }
        let gltf = gltf::Gltf::from_slice(bytes).map_err(|e| format!("bad GLB: {e}"))?;
        let blob = gltf.blob.as_deref().unwrap_or(&[]);
        check_buffers(&gltf.document, blob)?;
        Loader::new(&gltf.document, blob).load()
    }

    pub fn clip(&self, name: &str) -> Option<&Clip> {
        self.clips.iter().find(|clip| clip.name == name)
    }

    pub fn socket(&self, name: &str) -> Option<&Socket> {
        self.sockets.iter().find(|socket| socket.name == name)
    }

    /// The socket `name` as the creature shows it: drawn mirrored (facing
    /// the other way), its `_L` socket is the one on the `_R` side.
    pub fn socket_mirrored(&self, name: &str, mirrored: bool) -> Option<&Socket> {
        if mirrored {
            self.socket(&mirror_name(name))
        } else {
            self.socket(name)
        }
    }
}

/// `name` with its `_L` and `_R` suffix swapped; unsided names stay.
pub fn mirror_name(name: &str) -> String {
    if let Some(stem) = name.strip_suffix("_L") {
        format!("{stem}_R")
    } else if let Some(stem) = name.strip_suffix("_R") {
        format!("{stem}_L")
    } else {
        name.to_string()
    }
}

/// Refuses buffers that are not the binary chunk, and any view or accessor
/// reaching past its data, so that reading them later cannot panic.
fn check_buffers(document: &gltf::Document, blob: &[u8]) -> Result<(), String> {
    for buffer in document.buffers() {
        if !matches!(buffer.source(), gltf::buffer::Source::Bin) {
            return Err(format!(
                "buffer {} is not in the GLB's binary chunk",
                buffer.index()
            ));
        }
        if buffer.length() > blob.len() {
            return Err(format!(
                "buffer {} says {} bytes, the binary chunk has {}",
                buffer.index(),
                buffer.length(),
                blob.len()
            ));
        }
    }
    for view in document.views() {
        let end = view.offset().checked_add(view.length());
        if end.is_none_or(|end| end > view.buffer().length()) {
            return Err(format!("buffer view {} runs past its buffer", view.index()));
        }
    }
    for accessor in document.accessors() {
        if accessor.sparse().is_some() {
            return Err(format!(
                "accessor {}: sparse accessors are not supported",
                accessor.index()
            ));
        }
        let Some(view) = accessor.view() else {
            return Err(format!("accessor {} has no buffer view", accessor.index()));
        };
        let size = accessor.size();
        let stride = view.stride().unwrap_or(size);
        let needed = match accessor.count() {
            0 => Some(0),
            count => (count - 1)
                .checked_mul(stride)
                .and_then(|n| n.checked_add(size))
                .and_then(|n| n.checked_add(accessor.offset())),
        };
        if needed.is_none_or(|needed| needed > view.length()) {
            return Err(format!(
                "accessor {} runs past its buffer view",
                accessor.index()
            ));
        }
    }
    Ok(())
}

/// Refuses an accessor laid out other than `dimensions` of one of `types`:
/// the `gltf` readers panic on a layout they do not expect.
fn check_layout(
    accessor: Option<gltf::Accessor<'_>>,
    what: &str,
    dimensions: &[Dimensions],
    types: &[DataType],
) -> Result<(), String> {
    match accessor {
        Some(a) if !dimensions.contains(&a.dimensions()) || !types.contains(&a.data_type()) => {
            Err(format!(
                "{what}: {:?} of {:?} is not a layout glTF allows there",
                a.dimensions(),
                a.data_type()
            ))
        }
        _ => Ok(()),
    }
}

struct Loader<'a> {
    document: &'a gltf::Document,
    blob: &'a [u8],
    nodes: Vec<gltf::Node<'a>>,
    /// Each node's parent node.
    parents: Vec<Option<usize>>,
}

impl<'a> Loader<'a> {
    fn new(document: &'a gltf::Document, blob: &'a [u8]) -> Self {
        let mut parents = vec![None; document.nodes().len()];
        for node in document.nodes() {
            for child in node.children() {
                parents[child.index()] = Some(node.index());
            }
        }
        Self {
            document,
            blob,
            nodes: document.nodes().collect(),
            parents,
        }
    }

    fn buffer(&self) -> impl Fn(gltf::Buffer<'_>) -> Option<&'a [u8]> + Clone {
        let blob = self.blob;
        move |_| Some(blob)
    }

    fn node(&self, index: usize) -> &gltf::Node<'a> {
        &self.nodes[index]
    }

    fn name(&self, index: usize) -> String {
        self.node(index).name().unwrap_or("").to_string()
    }

    fn local(&self, index: usize) -> Transform {
        let (t, r, s) = self.node(index).transform().decomposed();
        Transform::new(
            Vec3::from(t),
            Quat::from_array(r).normalize(),
            Vec3::from(s),
        )
    }

    /// The node's world matrix at rest.
    fn world(&self, index: usize) -> Mat4 {
        let mut matrix = Mat4::IDENTITY;
        let mut at = Some(index);
        let mut steps = 0;
        while let Some(node) = at {
            matrix = self.local(node).matrix() * matrix;
            at = self.parents[node];
            steps += 1;
            if steps > self.parents.len() {
                break;
            }
        }
        matrix
    }

    fn load(&self) -> Result<CreatureAsset, String> {
        let skin = self
            .document
            .skins()
            .find(|skin| skin.joints().next().is_some())
            .ok_or("no skin: the creature needs an armature with bones")?;
        let (skeleton, bones) = self.skeleton(&skin)?;
        // The joint each bone node is.
        let joint_of = |node: usize| bones.iter().position(|&bone| bone == node);
        let primitives = self.primitives(&skin, &skeleton, &joint_of)?;
        let sockets = self.sockets(&joint_of)?;
        let clips = self.clips(&joint_of)?;
        let mut extras = BTreeMap::new();
        for node in self.document.nodes() {
            let value = extras_of(node.extras())?;
            if let (Some(name), false) = (node.name(), value.is_null()) {
                extras.insert(name.to_string(), value);
            }
        }
        let scene_extras = match self
            .document
            .default_scene()
            .or_else(|| self.document.scenes().next())
        {
            Some(scene) => extras_of(scene.extras())?,
            None => Value::Null,
        };
        Ok(CreatureAsset {
            primitives,
            skeleton,
            sockets,
            clips,
            extras,
            scene_extras,
        })
    }

    /// The skin's joints, then the other bones under the armature, with the
    /// node of each.
    fn skeleton(&self, skin: &gltf::Skin<'a>) -> Result<(Skeleton, Vec<usize>), String> {
        let mut nodes: Vec<usize> = skin.joints().map(|joint| joint.index()).collect();
        let skin_joints = nodes.len();
        let mut seen: HashSet<usize> = nodes.iter().copied().collect();
        if seen.len() != skin_joints {
            return Err("the skin lists a joint twice".to_string());
        }
        let tops: Vec<usize> = nodes
            .iter()
            .copied()
            .filter(|&n| self.parents[n].is_none_or(|p| !seen.contains(&p)))
            .collect();
        let armature = tops.first().and_then(|&top| self.parents[top]);
        if tops.iter().any(|&top| self.parents[top] != armature) {
            return Err("the skin's top bones hang from different nodes".to_string());
        }
        // The other bones: nodes under the armature that are neither meshes,
        // sockets, cameras nor lights (a mesh's children are not walked).
        let mut stack: Vec<usize> = match armature {
            Some(armature) => self.node(armature).children().map(|c| c.index()).collect(),
            None => tops.clone(),
        };
        stack.reverse();
        // A malformed file may loop its nodes: walk each one once.
        let mut walked = HashSet::new();
        while let Some(index) = stack.pop() {
            if !walked.insert(index) {
                continue;
            }
            let node = self.node(index);
            let is_bone = node.mesh().is_none()
                && node.camera().is_none()
                && !node.name().unwrap_or("").starts_with(SOCKET_PREFIX);
            if !is_bone {
                continue;
            }
            if seen.insert(index) {
                nodes.push(index);
            }
            let mut children: Vec<usize> = node.children().map(|c| c.index()).collect();
            children.reverse();
            stack.extend(children);
        }

        check_layout(
            skin.inverse_bind_matrices(),
            "inverse bind matrices",
            &[Dimensions::Mat4],
            &[DataType::F32],
        )?;
        let inverse_binds: Vec<Mat4> = match skin.reader(self.buffer()).read_inverse_bind_matrices()
        {
            Some(read) => read.map(|m| Mat4::from_cols_array_2d(&m)).collect(),
            None => vec![Mat4::IDENTITY; skin_joints],
        };
        if inverse_binds.len() != skin_joints {
            return Err(format!(
                "the skin has {skin_joints} joints but {} inverse bind matrices",
                inverse_binds.len()
            ));
        }
        let root = armature.map_or(Mat4::IDENTITY, |armature| self.world(armature));
        let joints = nodes
            .iter()
            .enumerate()
            .map(|(i, &node)| {
                let parent = self.parents[node].and_then(|p| nodes.iter().position(|&n| n == p));
                Ok(Joint {
                    name: self.name(node),
                    parent,
                    rest: self.local(node),
                    inverse_bind: inverse_binds
                        .get(i)
                        .copied()
                        .unwrap_or_else(|| self.world(node).inverse()),
                    extras: extras_of(self.node(node).extras())?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok((Skeleton::new(joints, skin_joints, root)?, nodes))
    }

    fn primitives(
        &self,
        skin: &gltf::Skin<'a>,
        skeleton: &Skeleton,
        joint_of: &dyn Fn(usize) -> Option<usize>,
    ) -> Result<Vec<Primitive>, String> {
        let mut primitives = Vec::new();
        for node in self.document.nodes() {
            let Some(mesh) = node.mesh() else { continue };
            let name = node.name().or(mesh.name()).unwrap_or("").to_string();
            if node
                .skin()
                .is_some_and(|other| other.index() != skin.index())
            {
                return Err(format!(
                    "mesh `{name}` uses another skin than the creature's"
                ));
            }
            for primitive in mesh.primitives() {
                if primitive.mode() != Mode::Triangles {
                    return Err(format!("mesh `{name}`: only triangle lists are supported"));
                }
                let mut primitive = self.primitive(&name, &node, &primitive, skeleton)?;
                if !primitive.is_skinned()
                    && let Some(joint) = self.bone_above(node.index(), joint_of)
                    && joint < skeleton.skin_joints
                    && let Ok(joint) = u16::try_from(joint)
                {
                    bind_whole(&mut primitive, joint);
                }
                primitives.push(primitive);
            }
        }
        if primitives.is_empty() {
            return Err("no mesh".to_string());
        }
        Ok(primitives)
    }

    fn primitive(
        &self,
        name: &str,
        node: &gltf::Node<'a>,
        primitive: &gltf::Primitive<'a>,
        skeleton: &Skeleton,
    ) -> Result<Primitive, String> {
        use DataType::{F32, U8, U16, U32};
        use gltf::Semantic;
        let unit = [F32, U8, U16];
        for (semantic, what, dimensions, types) in [
            (
                Semantic::Positions,
                "POSITION",
                &[Dimensions::Vec3][..],
                &[F32][..],
            ),
            (Semantic::Normals, "NORMAL", &[Dimensions::Vec3], &[F32]),
            (
                Semantic::TexCoords(0),
                "TEXCOORD_0",
                &[Dimensions::Vec2],
                &unit,
            ),
            (
                Semantic::Colors(0),
                "COLOR_0",
                &[Dimensions::Vec3, Dimensions::Vec4],
                &unit,
            ),
            (
                Semantic::Joints(0),
                "JOINTS_0",
                &[Dimensions::Vec4],
                &[U8, U16],
            ),
            (
                Semantic::Weights(0),
                "WEIGHTS_0",
                &[Dimensions::Vec4],
                &unit,
            ),
        ] {
            let what = format!("mesh `{name}`: {what}");
            check_layout(primitive.get(&semantic), &what, dimensions, types)?;
        }
        check_layout(
            primitive.indices(),
            &format!("mesh `{name}`: indices"),
            &[Dimensions::Scalar],
            &[U8, U16, U32],
        )?;
        let read = primitive.reader(self.buffer());
        let positions: Vec<Vec3> = read
            .read_positions()
            .ok_or_else(|| format!("mesh `{name}` has no positions"))?
            .map(Vec3::from)
            .collect();
        let count = positions.len();
        let normals: Vec<Vec3> = read
            .read_normals()
            .map(|r| r.map(Vec3::from).collect())
            .unwrap_or_default();
        let uv0: Vec<Vec2> = read
            .read_tex_coords(0)
            .map(|r| r.into_f32().map(Vec2::from).collect())
            .unwrap_or_default();
        let colors: Vec<Vec4> = read
            .read_colors(0)
            .map(|r| r.into_rgba_f32().map(Vec4::from).collect())
            .unwrap_or_default();
        let joints: Vec<[u16; 4]> = read
            .read_joints(0)
            .map(|r| r.into_u16().collect())
            .unwrap_or_default();
        let weights: Vec<Vec4> = read
            .read_weights(0)
            .map(|r| r.into_f32().map(Vec4::from).collect())
            .unwrap_or_default();
        let indices: Vec<u32> = match read.read_indices() {
            Some(r) => r.into_u32().collect(),
            None => (0..count as u32).collect(),
        };
        for (what, len) in [
            ("normals", normals.len()),
            ("TEXCOORD_0", uv0.len()),
            ("COLOR_0", colors.len()),
            ("JOINTS_0", joints.len()),
            ("WEIGHTS_0", weights.len()),
        ] {
            if len != 0 && len != count {
                return Err(format!("mesh `{name}`: {len} {what} for {count} vertices"));
            }
        }
        if joints.len() != weights.len() {
            return Err(format!("mesh `{name}`: JOINTS_0 and WEIGHTS_0 go together"));
        }
        if let Some(bad) = joints
            .iter()
            .flatten()
            .find(|&&j| j as usize >= skeleton.skin_joints)
        {
            return Err(format!("mesh `{name}`: joint {bad} is not in the skin"));
        }
        if let Some(bad) = indices.iter().find(|&&i| i as usize >= count) {
            return Err(format!(
                "mesh `{name}`: index {bad} past its {count} vertices"
            ));
        }
        if !indices.len().is_multiple_of(3) {
            return Err(format!(
                "mesh `{name}`: {} indices is not whole triangles",
                indices.len()
            ));
        }
        Ok(Primitive {
            mesh: name.to_string(),
            material: Material::of(&primitive.material()),
            positions,
            normals,
            uv0,
            colors,
            joints,
            weights,
            indices,
            world: self.world(node.index()),
        })
    }

    /// The nearest bone above `node`, as a joint.
    fn bone_above(&self, node: usize, joint_of: &dyn Fn(usize) -> Option<usize>) -> Option<usize> {
        let mut at = self.parents[node];
        let mut steps = 0;
        while let Some(index) = at {
            if let Some(joint) = joint_of(index) {
                return Some(joint);
            }
            at = self.parents[index];
            steps += 1;
            if steps > self.parents.len() {
                return None;
            }
        }
        None
    }

    fn sockets(&self, joint_of: &dyn Fn(usize) -> Option<usize>) -> Result<Vec<Socket>, String> {
        let mut sockets = Vec::new();
        for node in self.document.nodes() {
            let name = node.name().unwrap_or("");
            if !name.starts_with(SOCKET_PREFIX) {
                continue;
            }
            let joint = self.parents[node.index()]
                .and_then(joint_of)
                .ok_or_else(|| format!("socket `{name}` is not parented to a bone"))?;
            sockets.push(Socket {
                name: name.to_string(),
                joint,
                local: self.local(node.index()),
                extras: extras_of(node.extras())?,
            });
        }
        Ok(sockets)
    }

    fn clips(&self, joint_of: &dyn Fn(usize) -> Option<usize>) -> Result<Vec<Clip>, String> {
        let mut clips = Vec::new();
        for animation in self.document.animations() {
            let name = animation
                .name()
                .map(str::to_string)
                .unwrap_or_else(|| format!("animation {}", animation.index()));
            let mut channels = Vec::new();
            for channel in animation.channels() {
                // Channels on other nodes (meshes, sockets) are not the rig's.
                let Some(joint) = joint_of(channel.target().node().index()) else {
                    continue;
                };
                let sampler = channel.sampler();
                let outputs: (&[Dimensions], &[DataType]) = match channel.target().property() {
                    Property::Translation | Property::Scale => {
                        (&[Dimensions::Vec3], &[DataType::F32])
                    }
                    Property::Rotation => (
                        &[Dimensions::Vec4],
                        &[
                            DataType::F32,
                            DataType::I8,
                            DataType::U8,
                            DataType::I16,
                            DataType::U16,
                        ],
                    ),
                    // Morph target weights: not supported, skipped.
                    Property::MorphTargetWeights => continue,
                };
                let what = format!("clip `{name}`");
                check_layout(
                    Some(sampler.input()),
                    &what,
                    &[Dimensions::Scalar],
                    &[DataType::F32],
                )?;
                check_layout(Some(sampler.output()), &what, outputs.0, outputs.1)?;
                let read = channel.reader(self.buffer());
                let times: Vec<f32> = read
                    .read_inputs()
                    .ok_or_else(|| format!("clip `{name}`: a channel has no key times"))?
                    .collect();
                if times.windows(2).any(|pair| pair[1] < pair[0])
                    || times.iter().any(|t| !t.is_finite())
                {
                    return Err(format!("clip `{name}`: key times go backwards"));
                }
                let keys = match read.read_outputs() {
                    Some(ReadOutputs::Translations(v)) => {
                        Keys::Translation(v.map(Vec3::from).collect())
                    }
                    Some(ReadOutputs::Scales(v)) => Keys::Scale(v.map(Vec3::from).collect()),
                    Some(ReadOutputs::Rotations(v)) => {
                        Keys::Rotation(v.into_f32().map(Quat::from_array).collect())
                    }
                    // Morph target weights: not supported, skipped.
                    Some(ReadOutputs::MorphTargetWeights(_)) => continue,
                    None => return Err(format!("clip `{name}`: a channel has no values")),
                };
                let interpolation = match channel.sampler().interpolation() {
                    gltf::animation::Interpolation::Linear => Interpolation::Linear,
                    gltf::animation::Interpolation::Step => Interpolation::Step,
                    gltf::animation::Interpolation::CubicSpline => Interpolation::CubicSpline,
                };
                let per_key = if interpolation == Interpolation::CubicSpline {
                    3
                } else {
                    1
                };
                let values = match &keys {
                    Keys::Translation(v) | Keys::Scale(v) => v.len(),
                    Keys::Rotation(q) => q.len(),
                };
                if times.is_empty() || values != times.len() * per_key {
                    return Err(format!(
                        "clip `{name}`: {values} values for {} key times",
                        times.len()
                    ));
                }
                channels.push(Channel {
                    joint,
                    interpolation,
                    times,
                    keys,
                });
            }
            let duration = channels
                .iter()
                .filter_map(|c| c.times.last().copied())
                .fold(0.0, f32::max);
            clips.push(Clip {
                name,
                duration,
                channels,
            });
        }
        Ok(clips)
    }
}

/// An unskinned primitive parented to a bone, skinned whole to `joint`
/// so that it follows the bone (glTF would freeze it at rest otherwise).
/// Its vertices go to creature space at rest, where a skin's are.
fn bind_whole(primitive: &mut Primitive, joint: u16) {
    let world = primitive.world;
    let normal_world = glam::Mat3::from_mat4(world).inverse().transpose();
    for p in &mut primitive.positions {
        *p = world.transform_point3(*p);
    }
    for n in &mut primitive.normals {
        *n = (normal_world * *n).normalize_or(Vec3::Z);
    }
    let count = primitive.positions.len();
    primitive.joints = vec![[joint, 0, 0, 0]; count];
    primitive.weights = vec![Vec4::X; count];
}

fn extras_of(extras: &gltf::json::Extras) -> Result<Value, String> {
    match extras {
        Some(raw) => serde_json::from_str(raw.get()).map_err(|e| format!("bad extras: {e}")),
        None => Ok(Value::Null),
    }
}

#[cfg(test)]
mod tests;
