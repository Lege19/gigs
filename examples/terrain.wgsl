#import bevy_pbr::{
    mesh_bindings::mesh,
    mesh_view_bindings::globals,
    mesh_functions,
    skinning,
    morph::morph,
    forward_io::{Vertex, VertexOutput},
    view_transformations::position_world_to_clip,
}


@group(2) @binding(30) var<storage, read_write> old_heightmap: array<vec4<f32>>;
@group(2) @binding(31) var<storage, read_write> new_heightmap: array<vec4<f32>>;
@group(2) @binding(32) var<uniform> last_update: f32;

const INTERPOLATION_TIME: f32 = 0.5;

fn ease_out_expo(x: f32) -> f32 {
    return 1.0 - exp2(-10.0 * clamp(x, 0.0, 1.0));
}

@vertex
fn vertex(vertex: Vertex, @builtin(vertex_index) index: u32) -> VertexOutput {
    var out: VertexOutput;

    let old_heightmap_val = old_heightmap[index];
    let new_heightmap_val = new_heightmap[index];
    let dt = globals.time - last_update;
    let heightmap_val = mix(old_heightmap_val, new_heightmap_val, ease_out_expo(dt / INTERPOLATION_TIME));

    // Use vertex_no_morph.instance_index instead of vertex.instance_index to work around a wgpu dx12 bug.
    // See https://github.com/gfx-rs/naga/issues/2416 .
    var world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);

    out.world_normal = mesh_functions::mesh_normal_local_to_world(
        heightmap_val.xyz,
        // Use vertex_no_morph.instance_index instead of vertex.instance_index to work around a wgpu dx12 bug.
        // See https://github.com/gfx-rs/naga/issues/2416
        vertex.instance_index
    );

    out.world_position = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(vertex.position, 1.0));
    out.world_position.y += heightmap_val.w;

    out.position = position_world_to_clip(out.world_position.xyz);

    return out;
}
