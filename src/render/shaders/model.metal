// The creature's meshes: skinned on the GPU, lit with a soft stylized
// ramp (a toy, not a photograph): wrap diffuse, sky and ground ambient, a
// glossy lobe with a crisp highlight on smooth materials, and a rim that
// suits the desktop's theme.

struct VOut {
    float4 position [[position]];
    float3 normal;     // view space
    float2 uv0;
    float4 color;
};

vertex VOut model_vertex(uint vid [[vertex_id]],
                         const device Vertex* vertices [[buffer(0)]],
                         constant Globals& g [[buffer(1)]],
                         constant float4x4* skin [[buffer(2)]]) {
    Vertex v = vertices[vid];
    float3 p = float3(v.position);
    float3 n = float3(v.normal);

    float4 w = float4(v.weights);
    float4 j = float4(v.joints);
    float total = w.x + w.y + w.z + w.w;
    float4x4 M = float4x4(1.0);
    if (total > 1e-4) {
        w /= total;
        M = skin[int(j.x)] * w.x + skin[int(j.y)] * w.y + skin[int(j.z)] * w.z + skin[int(j.w)] * w.w;
    }
    float3x3 M3 = float3x3(M[0].xyz, M[1].xyz, M[2].xyz);
    // The cofactor matrix keeps normals right under squash and stretch.
    float3x3 cof = float3x3(cross(M3[1], M3[2]), cross(M3[2], M3[0]), cross(M3[0], M3[1]));
    float det = dot(M3[0], cross(M3[1], M3[2]));
    float3 skinned_n = cof * n * (det < 0.0 ? -1.0 : 1.0);

    float4 window = g.model * (M * float4(p, 1.0));
    VOut out;
    out.position = g.proj * (g.view * window);
    out.normal = normalize((g.normal_mat * float4(skinned_n, 0.0)).xyz);
    out.uv0 = float2(v.uv0);
    out.color = float4(v.color);
    return out;
}

fragment float4 model_fragment(VOut in [[stage_in]],
                               bool front [[front_facing]],
                               constant Globals& g [[buffer(0)]],
                               constant DrawInfo& d [[buffer(1)]]) {
    const float3 V = float3(0.0, 0.0, 1.0);
    float3 N = normalize(in.normal);
    if (!front && d.params.z > 0.5) N = -N;
    float3 L = g.key_dir.xyz;
    float3 Lb = g.back_dir.xyz;
    bool dark = g.misc.y > 0.5;

    float3 base = d.base_color.rgb;
    if (d.params.w > 0.5) base *= in.color.rgb;
    float rough = clamp(d.params.x, 0.05, 1.0);
    float metallic = saturate(d.params.y);
    float3 albedo = base * (1.0 - metallic);
    float3 f0 = mix(float3(0.04), base, metallic);
    float nv = saturate(dot(N, V));
    float nl = dot(N, L);

    // Wrap diffuse through a soft ramp: the shadow side stays coloured.
    float wrap = saturate((nl + 0.45) / 1.45);
    float ramp = mix(wrap, smoothstep(0.08, 0.92, wrap), 0.6);
    float3 color = albedo * g.key_col.rgb * g.key_dir.w * ramp;
    // Sky above, ground below, and a little of the back light.
    float up = N.y * 0.5 + 0.5;
    float3 hemi = mix(g.ground.rgb * g.ground.w, g.sky.rgb * g.sky.w, up);
    color += (albedo + f0 * metallic) * hemi;
    color += albedo * g.back_col.rgb * g.back_dir.w * 0.2 * saturate(dot(N, Lb));

    // A glossy lobe, and one crisp shaped highlight on smooth materials.
    float3 H = normalize(L + V);
    float nh = saturate(dot(N, H));
    float a = rough * rough;
    float3 fres = f0 + (1.0 - f0) * pow(1.0 - saturate(dot(V, H)), 5.0);
    color += g.key_col.rgb * ggx(nh, a) * smith_vis(saturate(nl), max(nv, 1e-3), a) * fres
             * saturate(nl);
    float gloss = 1.0 - smoothstep(0.25, 0.6, rough);
    float coat = smoothstep(0.55, 0.62, ggx_shape(nh, 0.24) * saturate(nl + 0.1));
    color += coat * 0.8 * gloss * g.key_col.rgb;

    // The edge, by theme: never an outline, the last pixel not the brightest.
    float f = pow(1.0 - nv, 3.5) * smoothstep(0.02, 0.12, nv);
    if (dark) {
        color += 0.7 * g.rim.rgb * f * saturate(nl + 0.6);
    } else {
        color *= 1.0 - 0.15 * f;
        color += 0.25 * g.rim.rgb * f * saturate(dot(N, Lb) + 0.3);
    }
    color += d.emissive.rgb;
    return float4(tonemap(color), 1.0);
}
