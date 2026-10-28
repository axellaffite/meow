"""Builds tests/fixtures/mini_creature.glb, the tiny creature the 3D tests
and examples use: four bones (one a non-deform ctrl_* data bone), one
socket, one skinned box with two materials (fur, and two eyes on its front), and
two clips.

Usage (from the repository root; tested with Blender 5.2):
    blender -b --factory-startup -P tools/blender/make_fixture.py -- tests/fixtures/mini_creature.glb

Blender meters, Z up, the creature facing -Y; the exporter writes glTF Y up,
facing +Z. The export settings in `export` are the contract the loader
expects (docs/3d.md): copy them for your own creature.
"""

import sys

import bmesh
import bpy
from mathutils import Matrix, Vector

FPS = 30
BONES = [
    # name, parent, head, tail, deform
    ("root", None, (0, 0, 0), (0, 0, 0.10), False),
    ("body", "root", (0, 0, 0.31), (0, 0, 0.40), True),
    ("head", "body", (0, 0, 0.45), (0, 0, 0.70), True),
    ("ctrl_face", "root", (0.6, 0, 0.70), (0.6, 0, 0.75), False),
]
# name, base colour (linear RGBA), roughness
MATERIALS = [
    ("fur", (0.9, 0.55, 0.25, 1.0), 0.8),
    ("eyes", (0.04, 0.035, 0.05, 1.0), 0.2),
]


def out_path():
    args = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
    if not args:
        raise SystemExit("usage: ... -- <out.glb>")
    return args[0]


def clear_scene():
    bpy.ops.wm.read_factory_settings(use_empty=True)
    scene = bpy.context.scene
    scene.render.fps = FPS
    scene.unit_settings.system = "METRIC"
    scene.unit_settings.scale_length = 1.0
    return scene


def make_rig(scene):
    data = bpy.data.armatures.new("rig")
    rig = bpy.data.objects.new("rig", data)
    scene.collection.objects.link(rig)
    rig["fixture"] = 1
    bpy.context.view_layer.objects.active = rig
    with bpy.context.temp_override(active_object=rig, object=rig):
        bpy.ops.object.mode_set(mode="EDIT")
    for name, parent, head, tail, deform in BONES:
        bone = data.edit_bones.new(name)
        bone.head = head
        bone.tail = tail
        bone.roll = 0.0
        bone.use_deform = deform
        if parent:
            bone.parent = data.edit_bones[parent]
    with bpy.context.temp_override(active_object=rig, object=rig):
        bpy.ops.object.mode_set(mode="OBJECT")
    for pose_bone in rig.pose.bones:
        pose_bone.rotation_mode = "XYZ"
    return rig


def make_socket(scene, rig):
    socket = bpy.data.objects.new("socket_head", None)
    socket.empty_display_type = "PLAIN_AXES"
    socket.empty_display_size = 0.03
    socket["purpose"] = "head"
    scene.collection.objects.link(socket)
    socket.parent = rig
    socket.parent_type = "BONE"
    socket.parent_bone = "head"
    bpy.context.view_layer.update()
    # Bone children hang from the tail: set the world matrix, not a location.
    socket.matrix_world = Matrix.Translation((0, 0, 1.06))
    return socket


def make_material(name, color, roughness):
    material = bpy.data.materials.new(name)
    material.diffuse_color = color
    material.roughness = roughness
    # A closed body: its back faces never show (glTF `doubleSided` false).
    material.use_backface_culling = True
    # Blender 5 gives every new material its node tree; older ones ask.
    if material.node_tree is None:
        material.use_nodes = True
    shader = material.node_tree.nodes.get("Principled BSDF")
    if shader is None:
        raise SystemExit(f"material {name}: no Principled BSDF")
    shader.inputs["Base Color"].default_value = color
    shader.inputs["Roughness"].default_value = roughness
    shader.inputs["Metallic"].default_value = 0.0
    return material


def make_body(scene, rig):
    mesh = bpy.data.meshes.new("body_mesh")
    bm = bmesh.new()
    bmesh.ops.create_cube(bm, size=1.0)
    # Each face cut in 3 x 3: two cells of the front become the eyes.
    bmesh.ops.subdivide_edges(bm, edges=bm.edges[:], cuts=2, use_grid_fill=True)
    for v in bm.verts:
        v.co = Vector((v.co.x * 0.6, v.co.y * 0.3, v.co.z * 0.7 + 0.35))
    bm.to_mesh(mesh)
    bm.free()
    body = bpy.data.objects.new("body_mesh", mesh)
    scene.collection.objects.link(body)

    for name, color, roughness in MATERIALS:
        mesh.materials.append(make_material(name, color, roughness))
    # The front's (normal -Y) top corner cells wear the eyes' material.
    for poly in mesh.polygons:
        c = poly.center
        eye = poly.normal.y < -0.5 and c.z > 0.5 and abs(c.x) > 0.1
        poly.material_index = 1 if eye else 0
        poly.use_smooth = False

    uvs = mesh.uv_layers.new(name="UVMap")
    for loop in mesh.loops:
        co = mesh.vertices[loop.vertex_index].co
        uvs.data[loop.index].uv = (co.x + 0.5, co.z)

    # Multiplies the base colour: the bottom a little darker. Its alpha
    # (0.25) only shows that COLOR_0 keeps all four channels.
    colors = mesh.color_attributes.new("Col", "FLOAT_COLOR", "POINT")
    for v in mesh.vertices:
        shade = 1.0 if v.co.z > 0.45 else 0.8
        colors.data[v.index].color = (shade, shade, shade, 0.25)
    mesh.color_attributes.active_color = colors

    lower = body.vertex_groups.new(name="body")
    upper = body.vertex_groups.new(name="head")
    for v in mesh.vertices:
        group = upper if v.co.z > 0.45 else lower
        group.add([v.index], 1.0, "REPLACE")

    body.parent = rig
    modifier = body.modifiers.new("Armature", "ARMATURE")
    modifier.object = rig
    return body


def fcurves_of(action, slot):
    layers = getattr(action, "layers", None)
    if layers:
        return layers[0].strips[0].channelbag(slot).fcurves
    return action.fcurves


def make_clip(rig, name, frames, keys):
    """`keys`: {bone: [(frame, location, rotation_euler, interpolation)]}."""
    action = bpy.data.actions.new(name)
    action.use_fake_user = True
    rig.animation_data_create()
    rig.animation_data.action = action
    if rig.animation_data.action_slot is None and hasattr(action, "slots"):
        rig.animation_data.action_slot = action.slots.new(id_type="OBJECT", name=rig.name)
    slot = rig.animation_data.action_slot
    # Every bone keyed at the first and last frame, so clips never leak.
    for pose_bone in rig.pose.bones:
        pose_bone.location = (0, 0, 0)
        pose_bone.rotation_euler = (0, 0, 0)
        pose_bone.scale = (1, 1, 1)
        for frame in (0, frames):
            for path in ("location", "rotation_euler", "scale"):
                pose_bone.keyframe_insert(path, frame=frame)
    modes = {}
    for bone, bone_keys in keys.items():
        pose_bone = rig.pose.bones[bone]
        for frame, location, rotation, interpolation in bone_keys:
            pose_bone.location = location
            pose_bone.rotation_euler = rotation
            pose_bone.keyframe_insert("location", frame=frame)
            pose_bone.keyframe_insert("rotation_euler", frame=frame)
            modes[(bone, frame)] = interpolation
    # One interpolation per curve: the exporter bakes mixed ones to linear.
    for fcurve in fcurves_of(action, slot):
        bone = fcurve.data_path.split('"')[1]
        constant = any(mode == "CONSTANT" for (b, _), mode in modes.items() if b == bone)
        for point in fcurve.keyframe_points:
            point.interpolation = "CONSTANT" if constant else "LINEAR"
    track = rig.animation_data.nla_tracks.new()
    track.name = name
    track.strips.new(name, 0, action)
    track.mute = True
    rig.animation_data.action = None


def export(path, objects):
    bpy.ops.object.select_all(action="DESELECT")
    for obj in objects:
        obj.select_set(True)
    wanted = {
        "export_format": "GLB",
        "use_selection": True,
        "export_yup": True,
        "export_apply": True,
        "export_texcoords": True,
        "export_normals": True,
        "export_tangents": False,
        "export_vertex_color": "ACTIVE",
        "export_materials": "EXPORT",
        "export_image_format": "NONE",
        "export_skins": True,
        "export_all_influences": False,
        "export_def_bones": False,
        "export_rest_position_armature": True,
        "export_hierarchy_flatten_bones": False,
        "export_animations": True,
        "export_animation_mode": "ACTIONS",
        "export_force_sampling": True,
        "export_frame_step": 1,
        "export_anim_slide_to_zero": True,
        "export_optimize_animation_size": False,
        "export_morph": False,
        "export_draco_mesh_compression_enable": False,
        "export_extras": True,
        "export_cameras": False,
        "export_lights": False,
    }
    known = bpy.ops.export_scene.gltf.get_rna_type().properties.keys()
    missing = [name for name in wanted if name not in known]
    if missing:
        raise SystemExit(f"glTF exporter parameters renamed: {missing}")
    bpy.ops.export_scene.gltf(filepath=path, **wanted)


def main():
    path = out_path()
    scene = clear_scene()
    rig = make_rig(scene)
    socket = make_socket(scene, rig)
    body = make_body(scene, rig)
    # idle: 30 frames, the head rises 0.006 m (along its bone, glTF +Y).
    make_clip(
        rig,
        "idle",
        30,
        {"head": [(15, (0, 0.006, 0), (0, 0, 0), "LINEAR")]},
    )
    # nod: 20 frames, the body leans and the head nods; ctrl_face holds
    # (3, 0.5, -0.25) from frame 10, constant. A bone pointing up with roll
    # 0 has the glTF axes, so its pose location is the data itself.
    make_clip(
        rig,
        "nod",
        20,
        {
            "ctrl_face": [
                (0, (0, 0, 0), (0, 0, 0), "CONSTANT"),
                (10, (3.0, 0.5, -0.25), (0, 0, 0), "CONSTANT"),
            ],
            "body": [(10, (0, 0, 0), (0.15, 0, 0), "LINEAR")],
            "head": [(10, (0, 0, 0), (0.3, 0, 0), "LINEAR")],
        },
    )
    export(path, [rig, socket, body])
    print(f"wrote {path}")


main()
