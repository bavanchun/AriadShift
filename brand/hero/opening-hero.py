"""Render the AriadShift opening hero: the brand glyph built in 3D and animated.

Run headless with Blender 5.2 LTS:

    Blender -b -P brand/hero/opening-hero.py -- --appearance light --out /path/to/frames

Then encode the PNG frames as HEVC with alpha at 60 fps (once per appearance, light and dark):

    ffmpeg -framerate 60 -i /path/to/frames/frame-%04d.png -c:v hevc_videotoolbox \
        -alpha_quality 0.9 -tag:v hvc1 -b:v 6M apps/macos/AriadShift/Opening/opening-hero-light.mov

The app plays apps/macos/AriadShift/Opening/opening-hero-light.mov and opening-hero-dark.mov.
Never edit those clips by hand; regenerate them from this script.

Regeneration is reproducible in look, not in bytes: EEVEE sampling and the VideoToolbox encoder
do not give byte-identical output across machines or versions, so the lint has no byte-level
--check for the clips. Review a regenerated clip by eye against the previous one.

The geometry mirrors brand/src/geometry.mjs (a 256-unit grid; the source and target
sheets are point-symmetric around (128,128) and the thread is one cubic half plus its
180-degree rotation). The camera is orthographic and straight down, so the last frame
registers 1:1 with the vector mark drawn in the same grid. The film is transparent: the
app keeps the green squircle tile as a vector backplate behind this clip.

Timing follows the opening animation spec at 60 fps (frame 1 = 0 ms, frame 53 = 860 ms):
sheets fade, scale from 0.94 and settle from a tilt, the source ring riding the raw sheet (0-160 ms); the thread draws along its
length with a travelling highlight at the stroke head (120-720 ms); the bead lands with
0 -> 1.12 -> 1 and a gold glow that blooms and fades (640-860 ms).
"""

import argparse
import math
import os
import sys

import bmesh
import bpy
from mathutils import Matrix, Vector
from mathutils.geometry import interpolate_bezier

UNIT = 0.01  # one geometry.mjs unit in scene metres; the grid spans 2.56 m
FPS = 60

SOURCE = (24, 22, 124, 156, 22, 0)  # x, y, w, h, r, ear
TARGET = (108, 78, 124, 156, 22, 38)
THREAD_WIDTH = 15
RING_R, RING_STROKE = 12.5, 8.5
BEAD_R = 14.5
S_HALF = ((122, 46), (58, 34), (42, 102), (100, 118))

Z_SOURCE, Z_TARGET, Z_THREAD = 0.0, 0.16, 0.32
SHEET_THICKNESS = 0.05

APPEARANCE = {
    # glass alpha floor (raw, finished), glass emission, gold base, world strength
    "light": {"alpha": (0.42, 0.58), "glass_emit": 0.08, "gold": "#FFC24A", "world": 1.1},
    "dark": {"alpha": (0.30, 0.44), "glass_emit": 0.04, "gold": "#F7B843", "world": 0.8},
}


def frame_at(ms):
    return 1 + round(ms * FPS / 1000)


def srgb(hex_color, alpha=1.0):
    h = hex_color.lstrip("#")
    lin = []
    for i in (0, 2, 4):
        c = int(h[i : i + 2], 16) / 255
        lin.append(c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4)
    return (*lin, alpha)


def grid(u, v, z=0.0):
    return Vector(((u - 128) * UNIT, -(v - 128) * UNIT, z))


def keyframes(owner, path, pairs, index=-1):
    for frame, value in pairs:
        if index >= 0:
            getattr(owner, path)[index] = value
        else:
            setattr(owner, path, value)
        owner.keyframe_insert(path, index=index, frame=frame)


def principled(material):
    material.use_nodes = True
    return next(n for n in material.node_tree.nodes if n.type == "BSDF_PRINCIPLED")


def reset_scene():
    scene = bpy.context.scene
    for collection in (bpy.data.objects, bpy.data.meshes, bpy.data.materials, bpy.data.curves,
                       bpy.data.lights, bpy.data.cameras, bpy.data.worlds):
        for block in list(collection):
            collection.remove(block)
    return scene


# ---------------------------------------------------------------- geometry

def sheet_outline(x, y, w, h, r, ear, steps=10):
    points = []

    def quad(p0, c, p1):
        for i in range(steps + 1):
            t = i / steps
            points.append(((1 - t) ** 2 * p0[0] + 2 * (1 - t) * t * c[0] + t * t * p1[0],
                           (1 - t) ** 2 * p0[1] + 2 * (1 - t) * t * c[1] + t * t * p1[1]))

    if ear:
        points += [(x + r, y), (x + w - ear, y), (x + w, y + ear)]
    else:
        points.append((x + r, y))
        quad((x + w - r, y), (x + w, y), (x + w, y + r))
    quad((x + w, y + h - r), (x + w, y + h), (x + w - r, y + h))
    quad((x + r, y + h), (x, y + h), (x, y + h - r))
    quad((x, y + r), (x, y), (x + r, y))
    unique = []
    for p in points:
        if not unique or abs(unique[-1][0] - p[0]) > 1e-6 or abs(unique[-1][1] - p[1]) > 1e-6:
            unique.append(p)
    if abs(unique[0][0] - unique[-1][0]) < 1e-6 and abs(unique[0][1] - unique[-1][1]) < 1e-6:
        unique.pop()
    return unique


def make_sheet(scene, name, spec, z):
    mesh = bpy.data.meshes.new(name)
    obj = bpy.data.objects.new(name, mesh)
    scene.collection.objects.link(obj)
    bm = bmesh.new()
    face = bm.faces.new([bm.verts.new(grid(u, v)) for u, v in sheet_outline(*spec)])
    bmesh.ops.recalc_face_normals(bm, faces=[face])
    if face.normal.z < 0:
        face.normal_flip()
    extruded = bmesh.ops.extrude_face_region(bm, geom=[face])
    for element in extruded["geom"]:
        if isinstance(element, bmesh.types.BMVert):
            element.co.z += SHEET_THICKNESS
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    bm.to_mesh(mesh)
    bm.free()
    centre = sum((v.co for v in mesh.vertices), Vector()) / len(mesh.vertices)
    mesh.transform(Matrix.Translation(-centre))
    obj.location = centre + Vector((0, 0, z))
    bevel = obj.modifiers.new("Bevel", "BEVEL")
    bevel.width, bevel.segments, bevel.limit_method = 0.018, 5, "ANGLE"
    obj.modifiers.new("WeightedNormal", "WEIGHTED_NORMAL")
    for polygon in mesh.polygons:
        polygon.use_smooth = True
    return obj


def make_ear(scene, target):
    x, y, w, _, _, ear = TARGET
    z = Z_TARGET + SHEET_THICKNESS + 0.004
    mesh = bpy.data.meshes.new("EarFold")
    mesh.from_pydata([grid(x + w - ear, y, z), grid(x + w - ear, y + ear, z + 0.03), grid(x + w, y + ear, z)],
                     [], [(0, 1, 2)])
    obj = bpy.data.objects.new("EarFold", mesh)
    scene.collection.objects.link(obj)
    obj.modifiers.new("Solidify", "SOLIDIFY").thickness = 0.012
    bpy.context.view_layer.update()  # matrix_world is stale until the depsgraph updates
    obj.parent = target
    obj.matrix_parent_inverse = target.matrix_world.inverted()
    return obj


def thread_points():
    p0, c1, c2, p1 = S_HALF
    rot = lambda p: (256 - p[0], 256 - p[1])
    q1, d2, d1, q0 = rot(p1), rot(c2), rot(c1), rot(p0)
    third = ((q1[0] - p1[0]) / 3, (q1[1] - p1[1]) / 3)
    return [
        (p0, p0, c1),
        (p1, c2, (p1[0] + third[0], p1[1] + third[1])),
        (q1, (q1[0] - third[0], q1[1] - third[1]), d2),
        (q0, d1, q0),
    ], p0, q0


def make_thread(scene, name, radius, z):
    curve = bpy.data.curves.new(name, "CURVE")
    curve.dimensions = "3D"
    spline = curve.splines.new("BEZIER")
    points, _, _ = thread_points()
    spline.bezier_points.add(len(points) - 1)
    for bp, (co, left, right) in zip(spline.bezier_points, points):
        bp.co, bp.handle_left, bp.handle_right = grid(*co, z), grid(*left, z), grid(*right, z)
        bp.handle_left_type = bp.handle_right_type = "FREE"
    curve.bevel_depth, curve.bevel_resolution, curve.resolution_u = radius, 8, 64
    curve.use_fill_caps = True
    curve.bevel_factor_mapping_end = "SPLINE"
    obj = bpy.data.objects.new(name, curve)
    scene.collection.objects.link(obj)
    return obj


def sample_thread(thread, count=200):
    bps = thread.data.splines[0].bezier_points
    points = []
    for i in range(len(bps) - 1):
        segment = interpolate_bezier(bps[i].co, bps[i].handle_right, bps[i + 1].handle_left, bps[i + 1].co, count)
        points += segment if not points else segment[1:]
    points = [thread.matrix_world @ p for p in points]
    lengths = [0.0]
    for a, b in zip(points, points[1:]):
        lengths.append(lengths[-1] + (b - a).length)

    def at(fraction):
        target = max(0.0, min(1.0, fraction)) * lengths[-1]
        for i in range(1, len(lengths)):
            if lengths[i] >= target:
                k = (target - lengths[i - 1]) / max(lengths[i] - lengths[i - 1], 1e-9)
                return points[i - 1].lerp(points[i], k)
        return points[-1]

    return at


# ---------------------------------------------------------------- materials

def glass_material(name, alpha_floor, roughness, emit):
    material = bpy.data.materials.new(name)
    material.use_nodes = True
    nt = material.node_tree
    for node in list(nt.nodes):
        nt.nodes.remove(node)
    output = nt.nodes.new("ShaderNodeOutputMaterial")
    bsdf = nt.nodes.new("ShaderNodeBsdfPrincipled")
    bsdf.inputs["Base Color"].default_value = srgb("#F4FFF2")
    bsdf.inputs["Roughness"].default_value = roughness
    bsdf.inputs["Coat Weight"].default_value = 1.0
    bsdf.inputs["Coat Roughness"].default_value = 0.03
    bsdf.inputs["Specular IOR Level"].default_value = 0.7
    bsdf.inputs["Emission Color"].default_value = srgb("#E8FFE6")
    bsdf.inputs["Emission Strength"].default_value = emit
    # Edges read denser than the face, like thick glass.
    weight = nt.nodes.new("ShaderNodeLayerWeight")
    weight.inputs["Blend"].default_value = 0.35
    remap = nt.nodes.new("ShaderNodeMapRange")
    remap.inputs["To Min"].default_value = alpha_floor
    remap.inputs["To Max"].default_value = 0.95
    fade = nt.nodes.new("ShaderNodeMath")
    fade.name, fade.operation = "Fade", "MULTIPLY"
    nt.links.new(weight.outputs["Fresnel"], remap.inputs["Value"])
    nt.links.new(remap.outputs["Result"], fade.inputs[0])
    nt.links.new(fade.outputs[0], bsdf.inputs["Alpha"])
    nt.links.new(bsdf.outputs["BSDF"], output.inputs["Surface"])
    material.surface_render_method = "BLENDED"
    material.use_transparency_overlap = False
    material.show_transparent_back = False
    return material


def gold_material(name, base, emit):
    material = bpy.data.materials.new(name)
    bsdf = principled(material)
    bsdf.inputs["Base Color"].default_value = srgb(base)
    bsdf.inputs["Metallic"].default_value = 1.0
    bsdf.inputs["Roughness"].default_value = 0.14
    bsdf.inputs["Anisotropic"].default_value = 0.2
    bsdf.inputs["Coat Weight"].default_value = 0.4
    bsdf.inputs["Emission Color"].default_value = srgb("#FFB23A")
    bsdf.inputs["Emission Strength"].default_value = emit
    return material


def flat_material(name, color, roughness):
    material = bpy.data.materials.new(name)
    bsdf = principled(material)
    bsdf.inputs["Base Color"].default_value = srgb(color)
    bsdf.inputs["Roughness"].default_value = roughness
    return material


def emission_material(name, color, strength):
    material = bpy.data.materials.new(name)
    material.use_nodes = True
    nt = material.node_tree
    for node in list(nt.nodes):
        nt.nodes.remove(node)
    output = nt.nodes.new("ShaderNodeOutputMaterial")
    emission = nt.nodes.new("ShaderNodeEmission")
    emission.inputs["Color"].default_value = srgb(color)
    emission.inputs["Strength"].default_value = strength
    nt.links.new(emission.outputs[0], output.inputs[0])
    return material


def glow_material(radius):
    material = bpy.data.materials.new("Glow")
    material.use_nodes = True
    nt = material.node_tree
    for node in list(nt.nodes):
        nt.nodes.remove(node)
    output = nt.nodes.new("ShaderNodeOutputMaterial")
    coords = nt.nodes.new("ShaderNodeTexCoord")
    mapping = nt.nodes.new("ShaderNodeMapping")
    mapping.inputs["Scale"].default_value = (1 / radius, 1 / radius, 1)
    gradient = nt.nodes.new("ShaderNodeTexGradient")
    gradient.gradient_type = "SPHERICAL"
    falloff = nt.nodes.new("ShaderNodeMath")
    falloff.operation = "POWER"
    falloff.inputs[1].default_value = 2.2
    amount = nt.nodes.new("ShaderNodeMath")
    amount.name, amount.operation = "GlowAmount", "MULTIPLY"
    emission = nt.nodes.new("ShaderNodeEmission")
    emission.inputs["Color"].default_value = srgb("#FFD27A")
    emission.inputs["Strength"].default_value = 4
    clear = nt.nodes.new("ShaderNodeBsdfTransparent")
    mix = nt.nodes.new("ShaderNodeMixShader")
    nt.links.new(coords.outputs["Object"], mapping.inputs["Vector"])
    nt.links.new(mapping.outputs["Vector"], gradient.inputs["Vector"])
    nt.links.new(gradient.outputs["Fac"], falloff.inputs[0])
    nt.links.new(falloff.outputs[0], amount.inputs[0])
    nt.links.new(amount.outputs[0], mix.inputs["Fac"])
    nt.links.new(clear.outputs[0], mix.inputs[1])
    nt.links.new(emission.outputs[0], mix.inputs[2])
    nt.links.new(mix.outputs[0], output.inputs["Surface"])
    material.surface_render_method = "BLENDED"
    return material


# ---------------------------------------------------------------- lighting and camera

def studio_world(scene, strength):
    """Bright warm studio for reflections only; the film is transparent, so it is never seen."""
    world = bpy.data.worlds.new("Studio")
    scene.world = world
    world.use_nodes = True
    nt = world.node_tree
    for node in list(nt.nodes):
        nt.nodes.remove(node)
    output = nt.nodes.new("ShaderNodeOutputWorld")
    background = nt.nodes.new("ShaderNodeBackground")
    coords = nt.nodes.new("ShaderNodeTexCoord")
    split = nt.nodes.new("ShaderNodeSeparateXYZ")
    remap = nt.nodes.new("ShaderNodeMapRange")
    remap.inputs["From Min"].default_value = -1
    ramp = nt.nodes.new("ShaderNodeValToRGB")
    ramp.color_ramp.elements[0].color = srgb("#1E3A2A")
    ramp.color_ramp.elements[1].color = srgb("#FFFBEF")
    ramp.color_ramp.elements.new(0.55).color = srgb("#F6E7C4")
    nt.links.new(coords.outputs["Normal"], split.inputs[0])
    nt.links.new(split.outputs["Z"], remap.inputs["Value"])
    nt.links.new(remap.outputs["Result"], ramp.inputs["Fac"])
    nt.links.new(ramp.outputs["Color"], background.inputs["Color"])
    background.inputs["Strength"].default_value = strength
    nt.links.new(background.outputs["Background"], output.inputs["Surface"])


def area_light(scene, name, location, energy, size, color):
    light = bpy.data.lights.new(name, "AREA")
    light.energy, light.size, light.color = energy, size, color
    obj = bpy.data.objects.new(name, light)
    scene.collection.objects.link(obj)
    obj.location = location
    obj.rotation_euler = (Vector((0, 0, 0.2)) - Vector(location)).to_track_quat("-Z", "Y").to_euler()


def point_light(scene, name, location, color, soft):
    light = bpy.data.lights.new(name, "POINT")
    light.color, light.shadow_soft_size, light.energy = color, soft, 0
    obj = bpy.data.objects.new(name, light)
    scene.collection.objects.link(obj)
    obj.location = location
    return obj


# ---------------------------------------------------------------- build

def build(appearance):
    look = APPEARANCE[appearance]
    scene = reset_scene()
    scene.render.engine = "BLENDER_EEVEE"
    scene.eevee.use_raytracing = True
    scene.eevee.taa_render_samples = 64
    scene.eevee.shadow_ray_count, scene.eevee.shadow_step_count = 3, 12
    scene.render.film_transparent = True
    scene.render.resolution_x = scene.render.resolution_y = 448
    scene.render.fps = FPS
    scene.frame_start, scene.frame_end = 1, frame_at(860)
    scene.view_settings.view_transform = "AgX"
    scene.render.image_settings.file_format = "PNG"
    scene.render.image_settings.color_mode = "RGBA"

    source = make_sheet(scene, "SourceSheet", SOURCE, Z_SOURCE)
    target = make_sheet(scene, "TargetSheet", TARGET, Z_TARGET)
    ear = make_ear(scene, target)
    raw = glass_material("GlassRaw", look["alpha"][0], 0.35, look["glass_emit"])
    finished = glass_material("GlassFinished", look["alpha"][1], 0.2, look["glass_emit"])
    source.data.materials.append(raw)
    target.data.materials.append(finished)
    ear.data.materials.append(finished)

    thread = make_thread(scene, "Thread", THREAD_WIDTH * UNIT / 2, Z_THREAD)
    contour = make_thread(scene, "ThreadContour", THREAD_WIDTH * UNIT / 2 * 1.38, Z_THREAD - 0.035)
    gold = gold_material("GoldThread", look["gold"], 0.25)
    bead_gold = gold_material("GoldBead", look["gold"], 0.5)
    thread.data.materials.append(gold)
    # The dark contour gives the thread an edge in grayscale and colour-vision deficiency.
    contour.data.materials.append(flat_material("Contour", "#3A2A0E", 0.35))

    _, start, end = thread_points()
    bpy.ops.mesh.primitive_torus_add(major_radius=RING_R * UNIT, minor_radius=RING_STROKE * UNIT / 2,
                                     major_segments=64, minor_segments=24, location=grid(*start, Z_THREAD))
    ring = bpy.context.active_object
    ring.name = "SourceRing"
    ring.data.materials.append(gold)
    bpy.ops.object.shade_smooth()
    bpy.ops.mesh.primitive_uv_sphere_add(radius=BEAD_R * UNIT, segments=64, ring_count=32,
                                         location=grid(*end, Z_THREAD))
    bead = bpy.context.active_object
    bead.name = "Bead"
    bead.data.materials.append(bead_gold)
    bpy.ops.object.shade_smooth()

    glow_radius = BEAD_R * UNIT * 1.6 * 2.2
    bpy.ops.mesh.primitive_plane_add(size=2 * glow_radius, location=bead.location - Vector((0, 0, 0.02)))
    glow = bpy.context.active_object
    glow.name = "BeadGlow"
    glow.data.materials.append(glow_material(glow_radius))

    bpy.ops.mesh.primitive_uv_sphere_add(radius=0.055, segments=32, ring_count=16)
    spark = bpy.context.active_object
    spark.name = "Spark"
    spark.data.materials.append(emission_material("Spark", "#FFF1C4", 30))
    spark_light = point_light(scene, "SparkLight", (0, 0, 0), (1, 0.82, 0.48), 0.05)
    spark_light.parent = spark
    flash = point_light(scene, "BeadFlash", bead.location + Vector((0, 0, 0.35)), (1, 0.8, 0.45), 0.2)

    studio_world(scene, look["world"])
    area_light(scene, "Key", (-3.0, 3.2, 4.0), 1300, 3.0, (1.0, 0.98, 0.94))
    area_light(scene, "Rim", (3.2, -3.0, 1.6), 500, 1.5, (1.0, 0.84, 0.55))
    area_light(scene, "Fill", (2.5, 3.0, 3.0), 160, 4.0, (0.85, 0.95, 1.0))
    camera_data = bpy.data.cameras.new("Camera")
    camera_data.type, camera_data.ortho_scale = "ORTHO", 256 * UNIT
    camera = bpy.data.objects.new("Camera", camera_data)
    scene.collection.objects.link(camera)
    camera.location = (0, 0, 10)
    scene.camera = camera

    animate(scene, source, target, raw, finished, thread, contour, ring, spark, spark_light,
            bead, bead_gold, glow, flash)


def animate(scene, source, target, raw, finished, thread, contour, ring, spark, spark_light,
            bead, bead_gold, glow, flash):
    f = frame_at
    slide = 6 / 112 * 256 * UNIT  # 6pt of the 112pt mark
    for sheet, direction in ((source, -1), (target, 1)):
        rest = sheet.location.copy()
        keyframes(sheet, "location", [(f(0), rest.x + direction * slide * 2.2), (f(160), rest.x)], 0)
        keyframes(sheet, "scale", [(f(0), (0.94,) * 3), (f(160), (1, 1, 1))])
        keyframes(sheet, "rotation_euler",
                  [(f(0), (math.radians(18), math.radians(22 * direction), 0)), (f(160), (0, 0, 0))])
    for material in (raw, finished):
        fade = material.node_tree.nodes["Fade"].inputs[1]
        keyframes(fade, "default_value", [(f(0), 0.0), (f(160), 1.0)])

    for curve_obj in (thread, contour):
        keyframes(curve_obj.data, "bevel_factor_end", [(f(120), 0.0), (f(720), 1.0)])
    # The ring belongs to the raw sheet: it travels, tilts and scales in with it (0-160 ms).
    scene.frame_set(f(160))
    bpy.context.view_layer.update()
    ring.parent = source
    ring.matrix_parent_inverse = source.matrix_world.inverted()
    # Gold cannot fade like glass, so the ring grows in while its sheet fades in.
    keyframes(ring, "scale", [(f(0), (0.001,) * 3), (f(160), (1, 1, 1))])

    # The highlight rides the stroke head: it reads the drawn fraction every frame.
    along = sample_thread(thread)
    for frame in range(scene.frame_start, scene.frame_end + 1):
        scene.frame_set(frame)
        spark.location = along(thread.data.bevel_factor_end) + Vector((0, 0, 0.02))
        spark.keyframe_insert("location", frame=frame)
    keyframes(spark, "scale", [(f(100), (0, 0, 0)), (f(160), (1, 1, 1)), (f(700), (1, 1, 1)), (f(760), (0, 0, 0))])
    keyframes(spark_light.data, "energy", [(f(100), 0), (f(160), 60), (f(700), 60), (f(780), 0)])

    keyframes(bead, "scale", [(f(640), (0, 0, 0)), (f(760), (1.12,) * 3), (f(860), (1, 1, 1))])
    emission = next(n for n in bead_gold.node_tree.nodes if n.type == "BSDF_PRINCIPLED").inputs["Emission Strength"]
    keyframes(emission, "default_value", [(f(640), 0.5), (f(740), 6.0), (f(860), 0.5)])
    glow_amount = glow.data.materials[0].node_tree.nodes["GlowAmount"].inputs[1]
    keyframes(glow_amount, "default_value", [(f(640), 0.0), (f(760), 0.35), (f(860), 0.0)])
    keyframes(glow, "scale", [(f(640), (0.6,) * 3), (f(760), (1, 1, 1)), (f(860), (1.15,) * 3)])
    keyframes(flash.data, "energy", [(f(640), 0), (f(750), 120), (f(860), 0)])
    scene.frame_set(scene.frame_start)


def main():
    argv = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--appearance", choices=sorted(APPEARANCE), default="light")
    parser.add_argument("--out", required=True, help="directory for the numbered RGBA PNG frames")
    parser.add_argument("--save-blend", help="also save the built scene to this .blend path")
    args = parser.parse_args(argv)
    build(args.appearance)
    os.makedirs(args.out, exist_ok=True)
    scene = bpy.context.scene
    scene.render.filepath = os.path.join(os.path.abspath(args.out), "frame-")
    if args.save_blend:
        bpy.ops.wm.save_as_mainfile(filepath=os.path.abspath(args.save_blend))
    bpy.ops.render.render(animation=True)


if __name__ == "__main__":
    main()
