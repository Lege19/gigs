use bevy::{
    asset::{embedded_asset, RenderAssetUsages},
    prelude::*,
};
use bevy_render::{
    mesh::{Indices, MeshVertexAttribute, MeshVertexAttributeId, PrimitiveTopology},
    render_resource::{
        AsBindGroup, BindGroupLayout, CommandEncoder, ComputePassDescriptor,
        ComputePipelineDescriptor, PipelineCache, ShaderType, SpecializedComputePipeline,
        VertexFormat,
    },
    renderer::RenderDevice,
    storage::ShaderStorageBuffer,
};
use gigs::{
    GraphicsJob, GraphicsJobsPlugin, InitGraphicsJobExt, JobAsBindGroup, JobComputePipeline,
    JobError, JobInputItem,
};

//TODO: spawn basic quad grid mesh, then have button to regenerate terrain with new seed. Have
//terrain height be a buffer
fn main() -> AppExit {
    let mut app = App::new();

    app.add_plugins(DefaultPlugins)
        .add_plugins(GraphicsJobsPlugin::default())
        .init_graphics_job::<TerrainGenJob>();

    embedded_asset!(app, "terrain_gen.wgsl");

    //TODO: add setup for actual scene and interactions for terrain gen

    app.run()
}

#[derive(ShaderType, Copy, Clone)]
struct TerrainParams {
    size: UVec2,
    resolution: UVec2,
}

//generates a simple flat grid mesh
fn generate_terrain_mesh(terrain_params: TerrainParams) -> Mesh {
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::all());

    mesh.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        (0..(terrain_params.resolution.x + 1) * (terrain_params.resolution.y + 1))
            .map(|i| {
                (
                    i / terrain_params.resolution.x,
                    i % terrain_params.resolution.x,
                )
            })
            .map(|(x, y)| {
                [
                    x as f32 / terrain_params.resolution.x as f32 * terrain_params.size.x as f32,
                    y as f32 / terrain_params.resolution.y as f32 * terrain_params.size.y as f32,
                    0.0,
                ]
            })
            .collect::<Vec<_>>(),
    );

    let mut indices: Vec<u32> = Vec::new();

    for y in 0..terrain_params.resolution.y {
        for x in 0..terrain_params.resolution.x {
            let i = y * terrain_params.resolution.y + x;
            indices.extend_from_slice(&[
                i,
                i + terrain_params.resolution.x,
                i + terrain_params.resolution.x + 1,
            ]);
            indices.extend_from_slice(&[i, i + terrain_params.resolution.x + 1, i + 1]);
        }
    }

    mesh.insert_indices(Indices::U32(indices));

    mesh.compute_smooth_normals();

    mesh
}

#[derive(AsBindGroup, Clone, Component)]
struct TerrainGenJob {
    #[storage(0)]
    heights_buffer: Handle<ShaderStorageBuffer>,
    #[uniform(1)]
    terrain_params: TerrainParams,
    #[uniform(1)]
    seed: f32,
}

#[derive(Resource)]
struct TerrainGenPipeline {
    layout: BindGroupLayout,
    shader: Handle<Shader>,
}

impl FromWorld for TerrainGenPipeline {
    fn from_world(world: &mut World) -> Self {
        let layout = TerrainGenJob::bind_group_layout(world.resource::<RenderDevice>());
        let shader = world
            .resource::<AssetServer>()
            .load("embedded://terrain_gen.wgsl");

        Self { layout, shader }
    }
}

impl SpecializedComputePipeline for TerrainGenPipeline {
    type Key = ();

    fn specialize(&self, (): Self::Key) -> ComputePipelineDescriptor {
        ComputePipelineDescriptor {
            label: Some("terrain_gen_compute".into()),
            layout: vec![self.layout.clone()],
            push_constant_ranges: Vec::new(),
            shader: self.shader.clone(),
            shader_defs: Vec::new(),
            entry_point: "main".into(),
            zero_initialize_workgroup_memory: false,
        }
    }
}

impl GraphicsJob for TerrainGenJob {
    type In = (JobAsBindGroup, JobComputePipeline<TerrainGenPipeline>);

    fn run(
        &self,
        _world: &World,
        _render_device: &RenderDevice,
        commands: &mut CommandEncoder,
        (job_bind_group, job_pipeline): JobInputItem<Self, Self::In>,
    ) -> Result<(), JobError> {
        let mut compute_pass = commands.begin_compute_pass(&ComputePassDescriptor {
            label: Some("terrain_gen_compute_pass"),
            timestamp_writes: None,
        });

        const WORKGROUP_SIZE: u32 = 16;

        compute_pass.set_bind_group(0, &job_bind_group.bind_group, &[]);
        compute_pass.set_pipeline(job_pipeline);
        compute_pass.dispatch_workgroups(
            self.terrain_params.resolution.x.div_ceil(WORKGROUP_SIZE),
            self.terrain_params.resolution.y.div_ceil(WORKGROUP_SIZE),
            1,
        );

        Ok(())
    }
}
