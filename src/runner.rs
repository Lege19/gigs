use core::iter;
use core::mem;

use bevy_app::{App, Plugin};
use bevy_ecs::{
    component::Component,
    entity::{Entity, EntityHashMap},
    query::{With, Without},
    system::{Commands, Local, ParamSet, Query, Res, ResMut, Resource},
    world::{EntityRef, World},
};
use bevy_render::render_resource::CommandEncoder;
use bevy_render::render_resource::CommandEncoderDescriptor;
use bevy_render::renderer::RenderDevice;
use bevy_render::renderer::RenderQueue;

use super::JobExecutionSettings;
use super::{ComputedPriority, GraphicsJob, JobError, JobInput, JobPriority};

#[derive(Copy, Clone, Component)]
pub struct DynamicJob(ErasedJobFn);

impl DynamicJob {
    fn run(
        &self,
        entity: EntityRef,
        world: &World,
        render_device: &RenderDevice,
        commands: &mut CommandEncoder,
    ) -> Result<(), JobError> {
        (self.0)(entity, world, render_device, commands)
    }
}

type ErasedJobFn =
    fn(EntityRef, &World, &RenderDevice, &mut CommandEncoder) -> Result<(), JobError>;

pub fn erase_jobs<J: GraphicsJob>(
    query: Query<Entity, (With<J>, Without<DynamicJob>)>,
    mut commands: Commands,
) {
    let jobs_to_erase = query.iter().collect::<Vec<_>>();
    commands.insert_batch(
        jobs_to_erase
            .into_iter()
            .zip(iter::repeat(DynamicJob(erased_job::<J>))),
    );
}

fn erased_job<J: GraphicsJob>(
    entity: EntityRef,
    world: &World,
    render_device: &RenderDevice,
    commands: &mut CommandEncoder,
) -> Result<(), JobError> {
    let Some((job, input_data)) = entity.get_components::<(&J, <J::In as JobInput<J>>::Data)>()
    else {
        return Err(JobError);
    };

    let input = <J::In as JobInput<J>>::get(input_data, world);

    job.run(world, render_device, commands, input)
}

#[derive(Resource)]
struct CompletedJobs(Vec<(Entity, Result<(), JobError>)>);

#[allow(clippy::type_complexity)]
fn run_jobs(
    mut params: ParamSet<(
        (
            Query<(EntityRef, &DynamicJob, &ComputedPriority)>,
            Res<RenderDevice>,
            Res<JobExecutionSettings>,
            &World,
        ),
        (Res<RenderQueue>, ResMut<CompletedJobs>),
    )>,
    mut command_encoders: Local<Vec<CommandEncoder>>,
    mut local_completed: Local<Vec<(Entity, Result<(), JobError>)>>,
) {
    local_completed.clear();

    let (jobs, render_device, job_exec_settings, world) = params.p0();

    let mut job_count: u32 = 0;
    let sorted_jobs =
        jobs.iter()
            .sort::<&ComputedPriority>()
            .rev()
            .take_while(|(_, _, priority)| {
                let cont =
                    priority.is_critical() || job_count < job_exec_settings.max_jobs_per_frame;
                job_count += 1;
                cont
            });

    for (entity_ref, job, _) in sorted_jobs {
        let mut commands =
            render_device.create_command_encoder(&CommandEncoderDescriptor { label: None });
        match job.run(entity_ref, world, &render_device, &mut commands) {
            Ok(()) => {
                command_encoders.push(commands);
                local_completed.push((entity_ref.id(), Ok(())));
            }
            Err(err) => local_completed.push((entity_ref.id(), Err(err))),
        }
    }

    let (render_queue, mut completed_jobs) = params.p1();
    mem::swap(&mut *local_completed, &mut completed_jobs.0);

    render_queue.submit(command_encoders.drain(..).map(|cmd| cmd.finish()));
}
