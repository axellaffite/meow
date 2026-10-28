// What is not the creature's body: the contact shadow, and the last pass
// that encodes the linear image to sRGB, premultiplied, into the window's
// target.

struct Flat {
    float4 position [[position]];
    float2 q;          // shape space
    float2 win;        // window points
};

constant float3 SHADOW = float3(0.0103, 0.0091, 0.0296);

vertex Flat shadow_vertex(uint vid [[vertex_id]], constant Globals& g [[buffer(1)]]) {
    float2 corner = float2(float(vid & 1u), float((vid >> 1) & 1u)) * 2.0 - 1.0;
    float2 radii = float2(g.shadow.z, g.shadow2.x);
    // On the ground line: the half in front of the creature shows.
    float2 centre = g.shadow.xy;
    float2 win = centre + corner * radii;
    Flat out;
    out.win = win;
    out.q = (win - centre) / max(radii, float2(1e-3));
    float2 window_size = g.viewport.xy / g.viewport.z;
    out.position = float4(win / window_size * 2.0 - 1.0, 0.5, 1.0);
    return out;
}

fragment float4 shadow_fragment(Flat in [[stage_in]], constant Globals& g [[buffer(0)]]) {
    float r2 = dot(in.q, in.q);
    float a = r2 < 1.0 ? (1.0 - r2) * (1.0 - r2) * g.shadow.w : 0.0;
    if (in.win.y < 0.0 || a <= 0.0) discard_fragment();
    return float4(SHADOW * a, a);
}

vertex Flat composite_vertex(uint vid [[vertex_id]]) {
    float2 corner = float2(float((vid << 1) & 2u), float(vid & 2u));
    Flat out;
    out.position = float4(corner * 2.0 - 1.0, 0.0, 1.0);
    out.q = corner;
    out.win = corner;
    return out;
}

// Linear premultiplied in, sRGB premultiplied out (straight colour encoded,
// then multiplied by alpha: what the window server blends). Fully empty
// pixels stay exactly zero. Light added without covering (colour above
// alpha) becomes a cover of that brightness, the one way a window can show
// it.
fragment float4 composite_fragment(Flat in [[stage_in]], texture2d<float> image [[texture(0)]]) {
    float4 c = image.read(uint2(in.position.xy));
    float3 rgb = max(c.rgb, 0.0);
    float a = max(saturate(c.a), saturate(max(rgb.r, max(rgb.g, rgb.b))));
    if (a <= 1.0 / 1024.0) return float4(0.0);
    float3 straight = saturate(rgb / a);
    float3 encoded = linear_to_srgb(straight);
    // A touch of dither against banding in the soft shadow.
    float noise = hash12(in.position.xy) - 0.5;
    encoded += noise / 255.0;
    return float4(saturate(encoded) * a, a);
}
