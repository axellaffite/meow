// The model's shaders, compiled at runtime (src/render/mod.rs) with
// newLibraryWithSource, so the build needs no Xcode. The files are joined in
// this order: common, model, post.
//
// Spaces: "creature" is glTF meters (Y up, facing +Z); "window" is points
// with y up, after `model`; "view" is window points pitched a few degrees
// down, looking along -Z, so V is always (0, 0, 1). Lighting is linear, each
// surface is tonemapped (Khronos PBR Neutral) as it is shaded, blended
// premultiplied, resolved in linear and encoded to sRGB by the last pass.

#include <metal_stdlib>
using namespace metal;

// Mirrors render::uniforms::Globals field for field.
struct Globals {
    float4x4 model;
    float4x4 view;
    float4x4 proj;
    float4x4 normal_mat;
    float4 viewport;   // px w, px h, px per pt, pt per m
    float4 misc;       // time, dark, quality, mirror
    float4 key_dir;    // xyz toward the light (view space), w intensity
    float4 key_col;
    float4 back_dir;   // w intensity
    float4 back_col;
    float4 sky;        // rgb, w intensity
    float4 ground;
    float4 rim;
    float4 shadow;     // centre x, ground y (window pt), half width (pt), alpha
    float4 shadow2;    // half height (pt)
};

// Mirrors render::uniforms::DrawInfo: one primitive's material.
struct DrawInfo {
    float4 base_color; // linear rgba
    float4 params;     // roughness, metallic, double sided, has vertex colours
    float4 emissive;   // linear rgb
};

struct Vertex {
    packed_float3 position;
    packed_float3 normal;
    packed_float2 uv0;
    packed_float4 color;
    packed_float4 joints;
    packed_float4 weights;
};

constant float PI = 3.14159265;

// Khronos PBR Neutral.
float3 tonemap(float3 color) {
    const float start = 0.8 - 0.04;
    const float desaturation = 0.15;
    float x = min(color.r, min(color.g, color.b));
    float offset = x < 0.08 ? x - 6.25 * x * x : 0.04;
    color -= offset;
    float peak = max(color.r, max(color.g, color.b));
    if (peak < start) return max(color, 0.0);
    const float d = 1.0 - start;
    float new_peak = 1.0 - d * d / (peak + d - start);
    color *= new_peak / peak;
    float g = 1.0 - 1.0 / (desaturation * (peak - new_peak) + 1.0);
    return mix(color, float3(new_peak), g);
}

float hash12(float2 p) {
    float3 p3 = fract(float3(p.xyx) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

// Normalized GGX: 1 at the peak, for shaped highlights.
float ggx_shape(float nh, float a) {
    float a2 = a * a;
    float d = nh * nh * (a2 - 1.0) + 1.0;
    return (a2 * a2) / (d * d);
}

float ggx(float nh, float a) {
    float a2 = a * a;
    float d = nh * nh * (a2 - 1.0) + 1.0;
    return a2 / (PI * d * d);
}

float smith_vis(float nl, float nv, float a) {
    float k = a * 0.5;
    return 0.25 / max((nl * (1.0 - k) + k) * (nv * (1.0 - k) + k), 1e-4);
}

float3 linear_to_srgb(float3 c) {
    c = saturate(c);
    return select(1.055 * pow(c, float3(1.0 / 2.4)) - 0.055, c * 12.92, c <= 0.0031308);
}
