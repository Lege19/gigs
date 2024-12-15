#import bevy_pbr::utils::rand_vec2f;

struct TerrainParams {
    size: vec2<u32>,
    resolution: vec2<u32>,
}

struct TerrainUniforms {
    params: TerrainParams,
    seed: f32,
    height_scale: f32
}

@group(0) @binding(0) var<storage, read_write> old_heightmap: array<vec4<f32>>;
@group(0) @binding(1) var<storage, read_write> new_heightmap: array<vec4<f32>>;
@group(0) @binding(2) var<uniform> uniforms: TerrainUniforms;

fn terrain_idx(coords: vec2<u32>) -> u32 {
    let clamped_coords = clamp(coords, vec2(0u), uniforms.params.resolution);
    return clamped_coords.y * uniforms.params.resolution.x + clamped_coords.x;
}

const NUM_OCTAVES: u32 = 16u;

// Credit to Inigo Quilez: https://iquilezles.org/articles/fbm/
// h: decay factor per octave
//
// x component: noise value
// yz components: noise gradient
fn fractal_noise(pos: vec2<f32>, h: f32) -> vec3<f32> {
    let G = exp2(-h);
    var f = 1.0;
    var a = 1.0;
    var t = vec3(0.0);
    for (var i: u32 = 0; i < NUM_OCTAVES; i++) {
        t += a * gradient_noise_d(f * pos);
        f *= 2.0;
        a *= G;
    }
    return t;
}

// Credit to Inigo Quilez: https://iquilezles.org/articles/gradientnoise/
//
// x component: noise value
// yz components: noise gradient
fn gradient_noise_d(pos: vec2<f32>) -> vec3<f32> {
    let i = floor(pos);
    let f = fract(pos);

    let u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let du = 30.0 * f * f * (f * (f - 2.0) + 1.0);

    let ga = hash(i + vec2(0.0, 0.0));
    let gb = hash(i + vec2(1.0, 0.0));
    let gc = hash(i + vec2(0.0, 1.0));
    let gd = hash(i + vec2(1.0, 1.0));

    let va = dot(ga, f - vec2(0.0, 0.0));
    let vb = dot(gb, f - vec2(1.0, 0.0));
    let vc = dot(gc, f - vec2(0.0, 1.0));
    let vd = dot(gd, f - vec2(1.0, 1.0));

    return vec3(va + u.x * (vb - va) + u.y * (vc - va) + u.x * u.y * (va - vb - vc + vd),   // value
        ga + u.x * (gb - ga) + u.y * (gc - ga) + u.x * u.y * (ga - gb - gc + gd) + // derivatives
                 du * (u.yx * (va - vb - vc + vd) + vec2(vb, vc) - va));
}

// Credit to Inigo Quilez: https://www.shadertoy.com/view/XdXBRH
fn hash(pos: vec2<f32>) -> vec2<f32> {
    const k: vec2<f32> = vec2(0.3183099, 0.3678794);
    var x = pos;
    x = x * k + k.yx;
    return -1.0 + 2.0 * fract(16.0 * k * fract(x.x * x.y * (x.x + x.y)));
}

@compute
@workgroup_size(16, 16, 1)
fn main(@builtin(global_invocation_id) coords: vec3<u32>) {
    if any(coords.xy > uniforms.params.resolution) { return; }

    var rand_state = bitcast<u32>(uniforms.seed);

    let pos = (vec2<f32>(coords.xy) / vec2<f32>(uniforms.params.resolution) + rand_vec2f(&rand_state));

    let height_gradient = fractal_noise(pos, 1.0) * uniforms.height_scale;
    let height = height_gradient.x;
    let normal = normalize(vec3(-height_gradient.y, 1.0, -height_gradient.z));

    new_heightmap[terrain_idx(coords.xy)] = vec4(normal, height);
}
