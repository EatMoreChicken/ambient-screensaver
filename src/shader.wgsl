struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) alpha: f32,
    @location(3) card_uv: vec2<f32>,
    @location(4) arch_height: f32,
    @location(5) card_aspect: f32,
    @location(6) mask_bits: f32,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) alpha: f32,
    @location(2) card_uv: vec2<f32>,
    @location(3) arch_height: f32,
    @location(4) card_aspect: f32,
    @location(5) mask_bits: f32,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    output.position = vec4<f32>(input.position, 0.0, 1.0);
    output.uv = input.uv;
    output.alpha = input.alpha;
    output.card_uv = input.card_uv;
    output.arch_height = input.arch_height;
    output.card_aspect = input.card_aspect;
    output.mask_bits = input.mask_bits;
    return output;
}

@group(0) @binding(0) var photo: texture_2d<f32>;
@group(0) @binding(1) var photo_sampler: sampler;

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(photo, photo_sampler, input.uv);
    let x = input.card_uv.x * 2.0 - 1.0;
    let y = input.card_uv.y / max(input.arch_height, 0.0001) - 1.0;
    let ellipse = x * x + y * y - 1.0;
    let edge = max(fwidth(ellipse), 0.0001);
    var mask = 1.0;
    if input.arch_height > 0.0 && input.card_uv.y < input.arch_height {
        mask = 1.0 - smoothstep(-edge, edge, ellipse);
    }
    let cut_u = 0.18 * min(1.0, 1.0 / input.card_aspect);
    let cut_v = 0.18 * min(1.0, input.card_aspect);
    let u = input.card_uv.x;
    let v = input.card_uv.y;
    let cuts = vec4<f32>(
        u / cut_u + v / cut_v - 1.0,
        (1.0 - u) / cut_u + v / cut_v - 1.0,
        (1.0 - u) / cut_u + (1.0 - v) / cut_v - 1.0,
        u / cut_u + (1.0 - v) / cut_v - 1.0,
    );
    let cut_edges = max(fwidth(cuts), vec4<f32>(0.0001));
    let cut_alpha = smoothstep(-cut_edges, cut_edges, cuts);
    let corners = u32(input.mask_bits);
    if (corners & 1u) != 0u { mask *= cut_alpha.x; }
    if (corners & 2u) != 0u { mask *= cut_alpha.y; }
    if (corners & 4u) != 0u { mask *= cut_alpha.z; }
    if (corners & 8u) != 0u { mask *= cut_alpha.w; }
    let corner_radii = vec2<f32>(cut_u, cut_v);
    let round_offset = abs(input.card_uv - vec2<f32>(0.5)) - (vec2<f32>(0.5) - corner_radii);
    let round_distance = length(max(round_offset, vec2<f32>(0.0)) / corner_radii) - 1.0;
    let round_edge = max(fwidth(round_distance), 0.0001);
    if (corners & 16u) != 0u {
        mask *= 1.0 - smoothstep(-round_edge, round_edge, round_distance);
    }
    if (corners & 32u) != 0u {
        let paper = vec3<f32>(0.985, 0.98, 0.965);
        let inside_photo = u >= 0.055 && u <= 0.945 && v >= 0.055 && v <= 0.78;
        let rgb = select(paper, mix(paper, color.rgb, color.a), inside_photo);
        return vec4<f32>(rgb, input.alpha);
    }
    return vec4<f32>(color.rgb, color.a * input.alpha * mask);
}
