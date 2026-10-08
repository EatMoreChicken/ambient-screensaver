struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) alpha: f32,
    @location(3) card_uv: vec2<f32>,
    @location(4) arch_height: f32,
    @location(5) card_aspect: f32,
    @location(6) mask_bits: f32,
    @location(7) corner_radius: f32,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) alpha: f32,
    @location(2) card_uv: vec2<f32>,
    @location(3) arch_height: f32,
    @location(4) card_aspect: f32,
    @location(5) mask_bits: f32,
    @location(6) corner_radius: f32,
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
    output.corner_radius = input.corner_radius;
    return output;
}

@group(0) @binding(0) var photo: texture_2d<f32>;
@group(0) @binding(1) var photo_sampler: sampler;

fn rounded_rect_distance(p: vec2<f32>, center: vec2<f32>, half_size: vec2<f32>, radius: f32) -> f32 {
    let q = abs(p - center) - (half_size - vec2<f32>(radius));
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - radius;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(photo, photo_sampler, input.uv);
    let aspect = max(input.card_aspect, 0.0001);
    let p = vec2<f32>(input.card_uv.x * aspect, input.card_uv.y);
    let short_side = min(aspect, 1.0);
    let corners = u32(input.mask_bits);
    let right = p.x >= aspect * 0.5;
    let bottom = p.y >= 0.5;
    let corner = select(select(1u, 2u, right), select(8u, 4u, right), bottom);
    let radius = select(
        input.corner_radius,
        min(input.corner_radius * 2.0, short_side * 0.46),
        (corners & corner) != 0u
    );
    let distance = rounded_rect_distance(
        p, vec2<f32>(aspect * 0.5, 0.5), vec2<f32>(aspect * 0.5, 0.5), radius
    );
    let outline_edge = max(fwidth(distance), 0.0001);
    var mask = 1.0 - smoothstep(-outline_edge, outline_edge, distance);

    let x = input.card_uv.x * 2.0 - 1.0;
    let y = input.card_uv.y / max(input.arch_height, 0.0001) - 1.0;
    let ellipse = x * x + y * y - 1.0;
    let edge = max(fwidth(ellipse), 0.0001);
    if input.arch_height > 0.0 && input.card_uv.y < input.arch_height {
        mask *= 1.0 - smoothstep(-edge, edge, ellipse);
    }
    if (corners & 32u) != 0u {
        let paper = vec3<f32>(0.985, 0.98, 0.965);
        let inset = aspect * 0.055;
        let photo_min = vec2<f32>(inset, inset);
        let photo_max = vec2<f32>(aspect * 0.945, 0.78);
        let photo_distance = rounded_rect_distance(
            p, (photo_min + photo_max) * 0.5, (photo_max - photo_min) * 0.5,
            max(radius - inset, 0.0)
        );
        let photo_edge = max(fwidth(photo_distance), 0.0001);
        let photo_mask = 1.0 - smoothstep(-photo_edge, photo_edge, photo_distance);
        let rgb = mix(paper, mix(paper, color.rgb, color.a), photo_mask);
        return vec4<f32>(rgb, input.alpha * mask);
    }
    return vec4<f32>(color.rgb, color.a * input.alpha * mask);
}
