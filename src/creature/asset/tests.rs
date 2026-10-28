use super::*;
use crate::creature::pose::{Pose, socket_position};

/// Made by tools/blender/make_fixture.py: bones root, body, head and the
/// non-deform ctrl_face; socket_head on the head; a skinned box wearing fur
/// and, on its front, eyes; clips idle (30 frames) and nod (20).
const FIXTURE: &[u8] = include_bytes!("../../../tests/fixtures/mini_creature.glb");
const FPS: f32 = 30.0;

fn fixture() -> CreatureAsset {
    CreatureAsset::from_glb(FIXTURE).unwrap()
}

fn close(a: Vec3, b: Vec3) -> bool {
    (a - b).length() < 1e-4
}

#[test]
fn every_bone_is_read_with_its_parent_and_rest() {
    let asset = fixture();
    let names: Vec<&str> = asset
        .skeleton
        .joints
        .iter()
        .map(|j| j.name.as_str())
        .collect();
    assert_eq!(names, ["root", "body", "head", "ctrl_face"]);
    let skeleton = &asset.skeleton;
    let parent = |name| skeleton.joints[skeleton.find(name).unwrap()].parent;
    assert_eq!(parent("root"), None);
    assert_eq!(parent("body"), skeleton.find("root"));
    assert_eq!(parent("head"), skeleton.find("body"));
    assert_eq!(parent("ctrl_face"), skeleton.find("root"));
    // Blender (0.6, 0, 0.70) is glTF (0.6, 0.70, 0): Y up, nothing converted.
    let face = &skeleton.joints[skeleton.find("ctrl_face").unwrap()];
    assert!(
        close(face.rest.t, Vec3::new(0.6, 0.7, 0.0)),
        "{}",
        face.rest.t
    );
}

#[test]
fn rest_pose_skins_the_mesh_where_it_is() {
    let asset = fixture();
    let skin = Pose::rest(&asset.skeleton).skin(&asset.skeleton);
    assert_eq!(skin.len(), asset.skeleton.skin_joints);
    for matrix in &skin {
        assert!(matrix.abs_diff_eq(Mat4::IDENTITY, 1e-4), "{matrix}");
    }
}

#[test]
fn primitives_carry_every_attribute_and_their_material() {
    let asset = fixture();
    let names: Vec<&str> = asset
        .primitives
        .iter()
        .map(|p| p.material.name.as_str())
        .collect();
    assert_eq!(names, ["fur", "eyes"]);
    let fur = &asset.primitives[0].material;
    assert!(
        fur.base_color
            .abs_diff_eq(Vec4::new(0.9, 0.55, 0.25, 1.0), 1e-4),
        "{}",
        fur.base_color
    );
    assert!((fur.roughness - 0.8).abs() < 1e-4, "{}", fur.roughness);
    assert_eq!(fur.metallic, 0.0);
    assert!(!fur.double_sided);
    assert!((asset.primitives[1].material.roughness - 0.2).abs() < 1e-4);
    for primitive in &asset.primitives {
        let n = primitive.positions.len();
        assert!(n > 0);
        assert_eq!(primitive.mesh, "body_mesh");
        for len in [
            primitive.normals.len(),
            primitive.uv0.len(),
            primitive.colors.len(),
            primitive.joints.len(),
            primitive.weights.len(),
        ] {
            assert_eq!(len, n);
        }
        assert!(primitive.is_skinned());
        assert_eq!(primitive.indices.len() % 3, 0);
        for weights in &primitive.weights {
            assert!((weights.element_sum() - 1.0).abs() < 1e-4);
        }
        for color in &primitive.colors {
            assert!(
                (color.w - 0.25).abs() < 1e-3,
                "COLOR_0 keeps its alpha: {color}"
            );
        }
    }
}

#[test]
fn sockets_hang_from_their_bone() {
    let asset = fixture();
    let socket = asset.socket("socket_head").unwrap();
    assert_eq!(asset.skeleton.joints[socket.joint].name, "head");
    assert_eq!(socket.extras["purpose"], "head");
    let world = Pose::rest(&asset.skeleton).world(&asset.skeleton);
    let at = socket_position(&world, socket);
    assert!(close(at, Vec3::new(0.0, 1.06, 0.0)), "{at}");
}

#[test]
fn mirrored_lookups_swap_the_sides() {
    assert_eq!(mirror_name("socket_hand_L"), "socket_hand_R");
    assert_eq!(mirror_name("socket_cling_R"), "socket_cling_L");
    assert_eq!(mirror_name("socket_cling_top"), "socket_cling_top");
    let mut asset = fixture();
    let head = asset.skeleton.find("head").unwrap();
    for name in ["socket_hand_L", "socket_hand_R"] {
        asset.sockets.push(Socket {
            name: name.to_string(),
            joint: head,
            local: Transform::IDENTITY,
            extras: Value::Null,
        });
    }
    let shown = |name: &str, mirrored: bool| {
        asset
            .socket_mirrored(name, mirrored)
            .map(|socket| socket.name.as_str())
    };
    assert_eq!(shown("socket_hand_L", false), Some("socket_hand_L"));
    assert_eq!(shown("socket_hand_L", true), Some("socket_hand_R"));
    assert_eq!(shown("socket_head", true), Some("socket_head"));
}

#[test]
fn custom_properties_come_through_as_extras() {
    let asset = fixture();
    assert_eq!(asset.extras["rig"]["fixture"], 1);
    assert_eq!(asset.extras["socket_head"]["purpose"], "head");
}

#[test]
fn clips_have_their_length_and_sample_into_a_pose() {
    let asset = fixture();
    let idle = asset.clip("idle").unwrap();
    assert!(
        (idle.duration - 30.0 / FPS).abs() < 1e-4,
        "{}",
        idle.duration
    );
    let nod = asset.clip("nod").unwrap();
    assert!((nod.duration - 20.0 / FPS).abs() < 1e-4, "{}", nod.duration);

    let skeleton = &asset.skeleton;
    let head = skeleton.find("head").unwrap();
    let mut pose = Pose::rest(skeleton);
    idle.sample(15.0 / FPS, &mut pose);
    let rise = pose.locals[head].t - skeleton.joints[head].rest.t;
    assert!(close(rise, Vec3::new(0.0, 0.006, 0.0)), "{rise}");
}

#[test]
fn data_bones_round_trip_through_the_export() {
    let asset = fixture();
    let skeleton = &asset.skeleton;
    let nod = asset.clip("nod").unwrap();
    let face = skeleton.find("ctrl_face").unwrap();
    let channel = nod
        .channels
        .iter()
        .find(|c| c.joint == face && c.path() == ChannelPath::Translation)
        .unwrap();
    assert_eq!(
        channel.interpolation,
        Interpolation::Step,
        "constant keys stay steps"
    );

    let data_at = |frame: f32| {
        let mut pose = Pose::rest(skeleton);
        nod.sample(frame / FPS, &mut pose);
        pose.locals[face].t - skeleton.joints[face].rest.t
    };
    assert!(close(data_at(5.0), Vec3::ZERO), "{}", data_at(5.0));
    let expected = Vec3::new(3.0, 0.5, -0.25);
    assert!(close(data_at(10.0), expected), "{}", data_at(10.0));
    // Step holds between keys.
    assert!(close(data_at(14.5), expected), "{}", data_at(14.5));
}

#[test]
fn malformed_input_is_an_error() {
    assert!(CreatureAsset::from_glb(b"").is_err());
    assert!(CreatureAsset::from_glb(b"not a glb at all").is_err());
    for cut in [12, 100, FIXTURE.len() / 2, FIXTURE.len() - 1] {
        assert!(
            CreatureAsset::from_glb(&FIXTURE[..cut]).is_err(),
            "cut at {cut}"
        );
    }
    let missing = CreatureAsset::load(Path::new("/nonexistent/creature.glb")).unwrap_err();
    assert!(missing.contains("/nonexistent/creature.glb"), "{missing}");
}

/// A GLB from `json` and its binary chunk.
fn glb(json: &str, bin: &[u8]) -> Vec<u8> {
    let pad = |mut bytes: Vec<u8>, with: u8| {
        while !bytes.len().is_multiple_of(4) {
            bytes.push(with);
        }
        bytes
    };
    let json = pad(json.as_bytes().to_vec(), b' ');
    let bin = pad(bin.to_vec(), 0);
    let total = 12 + 8 + json.len() + 8 + bin.len();
    let mut out = Vec::new();
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&json);
    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&bin);
    out
}

/// One triangle skinned to `root`; `ctrl_x` hangs from `root` but is not in
/// the skin, and `socket_tip` hangs from `ctrl_x`.
fn hand_made(skinned: bool) -> Vec<u8> {
    let mut bin = Vec::new();
    for v in [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
        for c in v {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    for c in Mat4::IDENTITY.to_cols_array() {
        bin.extend_from_slice(&c.to_le_bytes());
    }
    let skin = if skinned { r#","skin":0"# } else { "" };
    let skins = if skinned {
        r#","skins":[{"joints":[1],"inverseBindMatrices":1}]"#
    } else {
        ""
    };
    let json = format!(
        r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"nodes":[0]}}],
"nodes":[{{"name":"rig","children":[1,4]}},{{"name":"root","children":[2]}},
{{"name":"ctrl_x","translation":[0.5,0.25,0],"children":[3]}},
{{"name":"socket_tip","translation":[0,0.1,0]}},{{"name":"body","mesh":0{skin}}}],
"meshes":[{{"primitives":[{{"attributes":{{"POSITION":0}}}}]}}],
"buffers":[{{"byteLength":{len}}}],
"bufferViews":[{{"buffer":0,"byteOffset":0,"byteLength":36}},{{"buffer":0,"byteOffset":36,"byteLength":64}}],
"accessors":[{{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]}},
{{"bufferView":1,"componentType":5126,"count":1,"type":"MAT4"}}]{skins}}}"#,
        len = bin.len(),
    );
    glb(&json, &bin)
}

#[test]
fn bones_outside_the_skin_are_found_under_the_armature() {
    let asset = CreatureAsset::from_glb(&hand_made(true)).unwrap();
    let skeleton = &asset.skeleton;
    assert_eq!(skeleton.skin_joints, 1);
    let names: Vec<&str> = skeleton.joints.iter().map(|j| j.name.as_str()).collect();
    assert_eq!(names, ["root", "ctrl_x"]);
    assert_eq!(skeleton.joints[1].parent, Some(0));
    let socket = asset.socket("socket_tip").unwrap();
    assert_eq!(socket.joint, 1);
    let world = Pose::rest(skeleton).world(skeleton);
    assert!(close(
        socket_position(&world, socket),
        Vec3::new(0.5, 0.35, 0.0)
    ));
    // No indices: the triangle list 0, 1, 2.
    assert_eq!(asset.primitives[0].indices, [0, 1, 2]);
    assert!(asset.clips.is_empty());
}

#[test]
fn nodes_in_a_loop_are_an_error_not_a_hang() {
    // `root` and `ctrl_x` are each other's child.
    let mut bin = Vec::new();
    for c in [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
        bin.extend_from_slice(&c.to_le_bytes());
    }
    for c in Mat4::IDENTITY.to_cols_array() {
        bin.extend_from_slice(&c.to_le_bytes());
    }
    let json = format!(
        r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"nodes":[0]}}],
"nodes":[{{"name":"rig","children":[1,3]}},{{"name":"root","children":[2]}},
{{"name":"ctrl_x","children":[1]}},{{"name":"body","mesh":0,"skin":0}}],
"meshes":[{{"primitives":[{{"attributes":{{"POSITION":0}}}}]}}],
"skins":[{{"joints":[1],"inverseBindMatrices":1}}],
"buffers":[{{"byteLength":{len}}}],
"bufferViews":[{{"buffer":0,"byteOffset":0,"byteLength":36}},{{"buffer":0,"byteOffset":36,"byteLength":64}}],
"accessors":[{{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]}},
{{"bufferView":1,"componentType":5126,"count":1,"type":"MAT4"}}]}}"#,
        len = bin.len(),
    );
    assert!(CreatureAsset::from_glb(&glb(&json, &bin)).is_err());
}

#[test]
fn a_creature_without_a_skin_is_refused() {
    let error = CreatureAsset::from_glb(&hand_made(false)).unwrap_err();
    assert!(error.contains("no skin"), "{error}");
}

#[test]
fn an_unnamed_material_is_white_and_opaque() {
    let asset = CreatureAsset::from_glb(&hand_made(true)).unwrap();
    let material = &asset.primitives[0].material;
    assert_eq!(material.name, "");
    assert_eq!(material.base_color, Vec4::ONE);
    assert_eq!(material.emissive, Vec3::ZERO);
}
