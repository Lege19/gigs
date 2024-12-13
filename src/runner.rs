use core::iter;
use core::mem;
use std::cmp::Ordering;
use std::num::NonZero;

use bevy_app::{App, Plugin};
use bevy_ecs::query::Or;
use bevy_ecs::query::QueryData;
use bevy_ecs::system::EntityCommands;
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
use bevy_render::sync_world::MainEntity;
use bevy_render::Extract;
use disqualified::ShortName;

use crate::JobComplete;
use crate::JobInputStatus;
use crate::JobMarker;
use crate::JobPriority;
use crate::JobStatus;

use super::JobExecutionSettings;
use super::{GraphicsJob, JobError, JobInput};

#[derive(Copy, Clone, Component)]
pub struct DynamicJob {
    run: fn(EntityRef, &World, &RenderDevice, &mut CommandEncoder) -> Result<(), JobError>,
    status: fn(EntityRef, &World) -> JobInputStatus,
    label: ShortName<'static>,
}

impl DynamicJob {
    pub fn new<J: GraphicsJob>() -> Self {
        let run = erased_run::<J>;
        let status = erased_status::<J>;
        let label = J::label();
        Self { run, status, label }
    }

    pub fn status(&self, entity: EntityRef, world: &World) -> JobInputStatus {
        (self.status)(entity, world)
    }

    pub fn run(
        &self,
        entity: EntityRef,
        world: &World,
        render_device: &RenderDevice,
        commands: &mut CommandEncoder,
    ) -> Result<(), JobError> {
        (self.run)(entity, world, render_device, commands)
    }

    pub fn label(&self) -> ShortName<'static> {
        self.label
    }
}

fn erased_run<J: GraphicsJob>(
    entity: EntityRef,
    world: &World,
    render_device: &RenderDevice,
    command_encoder: &mut CommandEncoder,
) -> Result<(), JobError> {
    let Some((job, input_data)) = entity.get_components::<(&J, <J::In as JobInput<J>>::Data)>()
    else {
        return Err(JobError::InputsNotSatisfied);
    };

    let input = <J::In as JobInput<J>>::get(input_data, world);

    job.run(world, render_device, command_encoder, input)
}

fn erased_status<J: GraphicsJob>(entity: EntityRef, world: &World) -> JobInputStatus {
    let Some(input_data) = entity.get_components::<<J::In as JobInput<J>>::Data>() else {
        return JobInputStatus::Fail;
    };

    <J::In as JobInput<J>>::status(input_data, world)
}

pub fn erase_jobs<J: GraphicsJob>(
    query: Query<Entity, (With<J>, Without<DynamicJob>)>,
    mut commands: Commands,
) {
    let jobs_to_erase = query.iter().collect::<Vec<_>>();
    commands.insert_batch(
        jobs_to_erase
            .into_iter()
            .zip(iter::repeat(DynamicJob::new::<J>())),
    );
}

#[derive(Component, Copy, Clone)]
pub(super) struct FramesStalled(u32);

pub(super) fn setup_job_misc_components(
    jobs: Query<
        Entity,
        (
            With<JobMarker>,
            Or<(Without<FramesStalled>, Without<JobStatus>)>,
        ),
    >,
    mut commands: Commands,
) {
    let to_insert = jobs
        .iter()
        .zip(iter::repeat((FramesStalled(0), JobStatus::Waiting)))
        .collect::<Vec<_>>();
    commands.insert_batch(to_insert);
}

pub(super) fn cancel_stalled_jobs(
    jobs: Query<(Entity, Option<&MainEntity>, &FramesStalled)>,
    exec_settings: Res<JobExecutionSettings>,
    mut completed_jobs: ResMut<CompletedJobs>,
    mut commands: Commands,
) {
    jobs.iter()
        .filter(|(_, _, frames)| (frames.0 > exec_settings.max_job_stall_frames))
        .for_each(|(id, main_id, _)| {
            completed_jobs
                .0
                .push((id, main_id.copied(), Err(JobError::Stalled)));
            commands.entity(id).despawn();
        });
}

pub(super) fn increment_frames_stalled(mut jobs: Query<&mut FramesStalled>) {
    jobs.iter_mut().for_each(|mut frames| frames.0 += 1);
}

pub(super) fn check_job_inputs(
    jobs: Query<(EntityRef, &DynamicJob, &JobStatus)>,
    world: &World,
    mut commands: Commands,
) {
    jobs.iter()
        .filter(|(_, _, status)| **status == JobStatus::Waiting)
        .for_each(|(entity, job, _)| match job.status(entity, world) {
            JobInputStatus::Ready => {commands.entity(entity.id()).insert(JobStatus::Ready); },
            JobInputStatus::Wait => {},
            JobInputStatus::Fail => {
                todo!("need to handle failure here, despite not having mutable access to CompletedJobs")
            },
        });
}

#[derive(Resource)]
pub(super) struct CompletedJobs(Vec<(Entity, Option<MainEntity>, Result<(), JobError>)>);

pub(super) fn sync_completed_jobs(
    mut completed_jobs: ResMut<CompletedJobs>,
    mut commands: Commands,
    mut main_commands: Extract<Commands>,
) {
    for (id, main_id, res) in completed_jobs.0.drain(..) {
        commands.trigger_targets(JobComplete(res), id);
        if let Some(mut entity) = commands.get_entity(id) {
            entity.despawn();
        }
        if let Some(main_id) = main_id {
            main_commands.trigger_targets(JobComplete(res), id);
            if let Some(mut entity) = commands.get_entity(main_id.id()) {
                entity.despawn();
            }
        }
    }
}

pub(super) fn run_jobs(
    mut params: ParamSet<(
        (
            Query<(
                EntityRef,
                Option<&MainEntity>,
                &DynamicJob,
                &JobPriority,
                &JobStatus,
            )>,
            Res<RenderDevice>,
            Res<JobExecutionSettings>,
            &World,
        ),
        (Res<RenderQueue>, ResMut<CompletedJobs>),
    )>,
    mut command_encoders: Local<Vec<CommandEncoder>>,
    mut local_completed: Local<Vec<(Entity, Option<MainEntity>, Result<(), JobError>)>>,
) {
    local_completed.clear();

    let (jobs, render_device, job_exec_settings, world) = params.p0();

    let sorted_jobs = jobs
        .iter()
        .sort::<&JobPriority>()
        .rev()
        .filter(|(_, _, _, _, status)| **status == JobStatus::Ready)
        .enumerate()
        .take_while(|(i, (_, _, _, priority, _))| {
            priority.is_critical() || (*i as u32) < job_exec_settings.max_jobs_per_frame
        })
        .map(|(_, a)| a);

    for (entity_ref, main_entity, job, _, _) in sorted_jobs {
        let mut command_encoder =
            render_device.create_command_encoder(&CommandEncoderDescriptor { label: None });
        match job.run(entity_ref, world, &render_device, &mut command_encoder) {
            Ok(()) => {
                command_encoders.push(command_encoder);
                local_completed.push((entity_ref.id(), main_entity.copied(), Ok(())));
            }
            Err(err) => local_completed.push((entity_ref.id(), main_entity.copied(), Err(err))),
        }
    }

    let (render_queue, mut completed_jobs) = params.p1();
    mem::swap(&mut *local_completed, &mut completed_jobs.0);

    render_queue.submit(command_encoders.drain(..).map(|cmd| cmd.finish()));
}
