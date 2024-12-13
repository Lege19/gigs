use bevy::{
    asset::{embedded_asset, RenderAssetUsages},
    pbr::{ExtendedMaterial, MaterialExtension},
    prelude::*,
};
use bevy_render::{
    mesh::{Indices, PrimitiveTopology},
    render_resource::{
        AsBindGroup, BindGroupLayout, CommandEncoder, ComputePassDescriptor,
        ComputePipelineDescriptor, ShaderRef, ShaderType, SpecializedComputePipeline,
    },
    renderer::RenderDevice,
    storage::ShaderStorageBuffer,
};

use gigs::*;

//TODO: spawn basic quad grid mesh, then have button to regenerate terrain with new seed. Have
//terrain height be a buffer
fn main() -> AppExit {
    let mut app = App::new();

    app.add_plugins((
        DefaultPlugins,
        GraphicsJobsPlugin::default(),
        MaterialPlugin::<ExtendedMaterial<StandardMaterial, TerrainMaterial>>::default(),
    ))
    .init_graphics_job::<TerrainGenJob>();

    //embedded_asset!(app, "terrain_gen.wgsl");

    app.add_systems(Startup, setup_scene);

    //TODO: add setup for actual scene and interactions for terrain gen

    app.run()
}

fn setup_scene(
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ExtendedMaterial<StandardMaterial, TerrainMaterial>>>,
    mut storage_buffers: ResMut<Assets<ShaderStorageBuffer>>,
    time: Res<Time>,
    mut commands: Commands,
) {
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(-2.0, 4.0, -2.0).looking_at(Vec3::new(2.0, 0.0, 2.0), Vec3::Y),
    ));

    let terrain_params = TerrainParams {
        size: UVec2::splat(4),
        resolution: UVec2::splat(256),
    };

    let mesh = meshes.add(generate_terrain_mesh(terrain_params));
    let material = materials.add(ExtendedMaterial {
        base: StandardMaterial {
            base_color: Color::oklch(0.5045, 0.1328, 148.29),
            perceptual_roughness: 0.8,
            ..Default::default()
        },
        extension: TerrainMaterial::new(terrain_params, &mut storage_buffers, &time),
    });

    commands.spawn((MeshMaterial3d(material), Mesh3d(mesh)));
}

#[derive(ShaderType, Copy, Clone)]
struct TerrainParams {
    size: UVec2,
    resolution: UVec2,
}

#[derive(AsBindGroup, Clone, Asset, TypePath)]
struct TerrainMaterial {
    #[storage(30)]
    old_heightmap: Handle<ShaderStorageBuffer>,
    #[storage(31)]
    new_heightmap: Handle<ShaderStorageBuffer>,
    #[uniform(32)]
    last_update: f32,
}

impl TerrainMaterial {
    pub fn new(
        terrain_params: TerrainParams,
        storage_buffers: &mut Assets<ShaderStorageBuffer>,
        time: &Time,
    ) -> Self {
        let data =
            vec![0u8; (terrain_params.size.x as usize + 1) * (terrain_params.size.y as usize + 1)];
        let old_heightmap = storage_buffers.add(ShaderStorageBuffer::new(
            &data[..],
            RenderAssetUsages::all(),
        ));
        let new_heightmap = storage_buffers.add(ShaderStorageBuffer::new(
            &data[..],
            RenderAssetUsages::all(),
        ));
        let last_update = time.elapsed_secs_wrapped();

        Self {
            old_heightmap,
            new_heightmap,
            last_update,
        }
    }
}

impl MaterialExtension for TerrainMaterial {
    fn vertex_shader() -> ShaderRef {
        ShaderRef::Path("terrain.wgsl".into())
    }
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
    #[uniform(1)]
    scale: f32,
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
        command_encoder: &mut CommandEncoder,
        (job_bind_group, job_pipeline): JobInputItem<Self, Self::In>,
    ) -> Result<(), JobError> {
        let mut compute_pass = command_encoder.begin_compute_pass(&ComputePassDescriptor {
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
