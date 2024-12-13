use core::iter;
use core::mem;
use std::cmp::Ordering;
use std::num::NonZero;

use bevy_app::{App, Plugin};
use bevy_ecs::query::QueryData;
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

use crate::Priority;

use super::JobExecutionSettings;
use super::{ComputedPriority, GraphicsJob, JobError, JobInput};

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

#[derive(QueryData)]
struct JobSortLens<'a> {
    priority: &'a ComputedPriority,
    stall: &'a StalledFrames,
}

impl PartialEq for JobSortLensItem<'_, '_> {
    fn eq(&self, other: &Self) -> bool {
        self.priority.priority() == other.priority.priority() && self.stall.0 == other.stall.0
    }
}

impl Eq for JobSortLensItem<'_, '_> {}

impl PartialOrd for JobSortLensItem<'_, '_> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for JobSortLensItem<'_, '_> {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self.priority.priority(), other.priority.priority()) {
            (Priority::Critical, Priority::Critical) => Ordering::Equal,
            (Priority::Critical, Priority::NonCritical(_)) => Ordering::Greater,
            (Priority::NonCritical(_), Priority::Critical) => Ordering::Less,
            (Priority::NonCritical(weight_l), Priority::NonCritical(weight_r)) => {
                let adjusted_left =
                    weight_l.saturating_mul(NonZero::<u32>::MIN.saturating_add(self.stall.0));
                let adjusted_right =
                    weight_r.saturating_mul(NonZero::<u32>::MIN.saturating_add(other.stall.0));
                adjusted_left.cmp(&adjusted_right)
            }
        }
    }
}

//TODO: insert/increment stalled frames
#[derive(Component)]
struct StalledFrames(u32);

#[derive(Resource)]
struct CompletedJobs(Vec<(Entity, Result<(), JobError>)>);

fn run_jobs(
    mut params: ParamSet<(
        (
            Query<(EntityRef, &DynamicJob, &ComputedPriority, &StalledFrames)>,
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

    let sorted_jobs = jobs
        .iter()
        .sort::<JobSortLens>()
        .rev()
        .enumerate()
        .take_while(|(i, (_, _, priority, _))| {
            priority.is_critical() || (*i as u32) < job_exec_settings.max_jobs_per_frame
        })
        .map(|(_, a)| a);

    for (entity_ref, job, _, _) in sorted_jobs {
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
