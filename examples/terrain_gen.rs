use std::mem;

use bevy::{
    asset::{embedded_asset, RenderAssetUsages},
    input::keyboard::KeyboardInput,
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
use input::{JobAsBindGroup, JobComputePipeline, JobInputItem};

fn main() -> AppExit {
    let mut app = App::new();

    app.add_plugins((
        DefaultPlugins,
        GraphicsJobsPlugin::default(),
        MaterialPlugin::<ExtendedMaterial<StandardMaterial, TerrainMaterial>>::default(),
    ))
    .init_graphics_job::<TerrainGenJob>();

    embedded_asset!(app, "examples", "terrain_gen.wgsl");
    embedded_asset!(app, "examples", "terrain.wgsl");

    app.add_systems(Startup, setup_scene)
        .add_systems(Update, handle_input);

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

    commands.spawn((
        DirectionalLight {
            shadows_enabled: false,
            ..Default::default()
        },
        Transform::from_xyz(1.0, 6.0, 2.0).looking_at(Vec3::new(2.0, 0.0, 2.0), Vec3::Y),
    ));

    commands.spawn((
        Text::from("Press [space] to generate new terrain!"),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(12.0),
            top: Val::Px(12.0),
            ..Default::default()
        },
    ));

    let terrain_params = TerrainParams {
        size: UVec2::splat(4),
        resolution: UVec2::splat(256),
    };

    let mesh = meshes.add(generate_terrain_mesh(terrain_params));
    let material = materials.add(ExtendedMaterial {
        base: StandardMaterial {
            base_color: Color::oklch(0.5045, 0.1328, 148.29),
            perceptual_roughness: 0.4,
            ..Default::default()
        },
        extension: TerrainMaterial::new(terrain_params, &mut storage_buffers, &time),
    });

    commands.spawn((MeshMaterial3d(material), Mesh3d(mesh)));
}

fn handle_input(
    mut keyboard_input: EventReader<KeyboardInput>,
    terrain: Single<&MeshMaterial3d<ExtendedMaterial<StandardMaterial, TerrainMaterial>>>,
    mut materials: ResMut<Assets<ExtendedMaterial<StandardMaterial, TerrainMaterial>>>,
    time: Res<Time>,
    mut commands: Commands,
) {
    let current_time = time.elapsed_secs_wrapped();
    let MeshMaterial3d(handle) = terrain.into_inner();
    if let Some(material) = materials.get_mut(handle) {
        let dt = current_time - material.extension.last_update;

        if keyboard_input
            .read()
            .any(|key| key.key_code == KeyCode::Space)
            && dt > 0.25
        {
            material.extension.last_update = current_time;
            mem::swap(
                &mut material.extension.old_heightmap,
                &mut material.extension.new_heightmap,
            );

            commands.spawn(TerrainGenJob {
                old_heightmap: material.extension.old_heightmap.clone(),
                new_heightmap: material.extension.new_heightmap.clone(),
                terrain_params: material.extension.terrain_params,
                seed: current_time,
                height_scale: 2.0,
            });
        }
    }
}

#[derive(ShaderType, Copy, Clone)]
struct TerrainParams {
    size: UVec2,
    resolution: UVec2,
}

impl TerrainParams {
    fn vertex_count(&self) -> u32 {
        self.resolution.x * self.resolution.y
    }
}

#[derive(AsBindGroup, Clone, Asset, TypePath)]
struct TerrainMaterial {
    #[storage(30, visibility(vertex))]
    old_heightmap: Handle<ShaderStorageBuffer>,
    #[storage(31, visibility(vertex))]
    new_heightmap: Handle<ShaderStorageBuffer>,
    #[uniform(32, visibility(vertex))]
    last_update: f32,
    terrain_params: TerrainParams,
}

impl TerrainMaterial {
    pub fn new(
        terrain_params: TerrainParams,
        storage_buffers: &mut Assets<ShaderStorageBuffer>,
        time: &Time,
    ) -> Self {
        let data = vec![0u8; terrain_params.vertex_count() as usize * size_of::<Vec4>()];
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
            terrain_params,
        }
    }
}

impl MaterialExtension for TerrainMaterial {
    fn vertex_shader() -> ShaderRef {
        ShaderRef::Path("embedded://terrain_gen/terrain.wgsl".into())
    }
}

//generates a simple flat grid mesh
fn generate_terrain_mesh(terrain_params: TerrainParams) -> Mesh {
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::all());

    let mut verts = Vec::new();

    for y in 0..terrain_params.resolution.y {
        for x in 0..terrain_params.resolution.x {
            let x_pos =
                x as f32 / terrain_params.resolution.x as f32 * terrain_params.size.x as f32;
            let z_pos =
                y as f32 / terrain_params.resolution.y as f32 * terrain_params.size.y as f32;
            verts.push([x_pos, 0.0, z_pos]);
        }
    }

    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, verts);

    mesh.insert_attribute(
        Mesh::ATTRIBUTE_NORMAL,
        vec![[0.0, 1.0, 0.0]; terrain_params.vertex_count() as usize],
    );

    let mut indices: Vec<u32> = Vec::new();

    for y in 0..terrain_params.resolution.y - 1 {
        for x in 0..terrain_params.resolution.x - 1 {
            let i = y * terrain_params.resolution.x + x;
            indices.extend_from_slice(&[
                i,
                i + terrain_params.resolution.x,
                i + terrain_params.resolution.x + 1,
            ]);
            indices.extend_from_slice(&[i, i + terrain_params.resolution.x + 1, i + 1]);
        }
    }

    mesh.insert_indices(Indices::U32(indices));

    mesh
}

#[derive(AsBindGroup, Clone, Component)]
#[require(JobComputePipeline<TerrainGenPipeline>)]
struct TerrainGenJob {
    #[storage(0, visibility(compute))]
    old_heightmap: Handle<ShaderStorageBuffer>,
    #[storage(1, visibility(compute))]
    new_heightmap: Handle<ShaderStorageBuffer>,
    #[uniform(2, visibility(compute))]
    terrain_params: TerrainParams,
    #[uniform(2, visibility(compute))]
    seed: f32,
    #[uniform(2, visibility(compute))]
    height_scale: f32,
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
            .load("embedded://terrain_gen/terrain_gen.wgsl");

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
