use core::iter;

use bevy_ecs::{
    component::Component,
    entity::Entity,
    query::{With, Without},
    system::{Commands, Local, Query, Res, Resource},
    world::{EntityRef, World},
};
use bevy_render::render_resource::CommandEncoder;
use bevy_render::render_resource::CommandEncoderDescriptor;
use bevy_render::renderer::RenderDevice;
use bevy_render::renderer::RenderQueue;
use bevy_render::sync_world::MainEntity;
use crossbeam_channel::Receiver;
use crossbeam_channel::Sender;
use disqualified::ShortName;

use crate::{JobComplete, JobInputStatus, JobMarker, JobPriority};

use super::JobExecutionSettings;
use super::{GraphicsJob, JobError, JobInput};

#[derive(Copy, Clone, Component)]
pub struct DynamicJob {
    label: ShortName<'static>,
    status: fn(EntityRef, &World) -> JobInputStatus,
    run: fn(EntityRef, &World, &RenderDevice, &mut CommandEncoder) -> Result<(), JobError>,
}

impl DynamicJob {
    pub fn new<J: GraphicsJob>() -> Self {
        let label = J::label();
        let status = erased_status::<J>;
        let run = erased_run::<J>;
        Self { label, status, run }
    }

    pub fn label(&self) -> ShortName<'static> {
        self.label
    }

    pub fn status(&self, entity: EntityRef, world: &World) -> JobInputStatus {
        (self.status)(entity, world)
    }

    pub fn run(
        &self,
        entity: EntityRef,
        world: &World,
        render_device: &RenderDevice,
        command_encoder: &mut CommandEncoder,
    ) -> Result<(), JobError> {
        (self.run)(entity, world, render_device, command_encoder)
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

pub(super) fn setup_stalled_frames(
    jobs: Query<Entity, (With<JobMarker>, Without<FramesStalled>)>,
    mut commands: Commands,
) {
    let to_insert = jobs
        .iter()
        .zip(iter::repeat(FramesStalled(0)))
        .collect::<Vec<_>>();
    commands.insert_batch(to_insert);
}

pub(super) fn cancel_stalled_jobs(
    jobs: Query<(Entity, Option<&MainEntity>, &FramesStalled)>,
    exec_settings: Res<JobExecutionSettings>,
    completed_jobs: Res<JobResultSender>,
    mut commands: Commands,
) {
    jobs.iter()
        .filter(|(_, _, frames)| (frames.0 > exec_settings.max_job_stall_frames))
        .for_each(|(id, main_id, _)| {
            completed_jobs
                .0
                .send(JobResult {
                    entity: id,
                    main_entity: main_id.copied(),
                    result: Err(JobError::Stalled),
                })
                .unwrap();
            commands.entity(id).despawn();
        });
}

pub(super) fn increment_frames_stalled(mut jobs: Query<&mut FramesStalled>) {
    jobs.iter_mut().for_each(|mut frames| frames.0 += 1);
}

#[derive(Copy, Clone, Component)]
pub struct JobReady;

pub(super) fn check_job_inputs(
    jobs: Query<(EntityRef, Option<&MainEntity>, &DynamicJob), Without<JobReady>>,
    world: &World,
    job_result_sender: Res<JobResultSender>,
    mut commands: Commands,
) {
    let to_insert = jobs
        .iter()
        .filter_map(
            |(entity, main_entity, job)| match job.status(entity, world) {
                JobInputStatus::Ready => Some(entity.id()),
                JobInputStatus::Wait => None,
                JobInputStatus::Fail => {
                    job_result_sender
                        .0
                        .send(JobResult {
                            entity: entity.id(),
                            main_entity: main_entity.copied(),
                            result: Err(JobError::InputsNotSatisfied),
                        })
                        .unwrap();
                    None
                }
            },
        )
        .zip(iter::repeat(JobReady))
        .collect::<Vec<_>>();
    commands.insert_batch(to_insert)
}

#[derive(Copy, Clone)]
pub(super) struct JobResult {
    entity: Entity,
    main_entity: Option<MainEntity>,
    result: Result<(), JobError>,
}

#[derive(Resource)]
pub(super) struct JobResultReceiver(pub Receiver<JobResult>);
#[derive(Resource)]
pub(super) struct JobResultSender(pub Sender<JobResult>);

#[derive(Resource)]
pub(super) struct JobResultMainWorldReceiver(pub Receiver<JobResult>);
#[derive(Resource)]
pub(super) struct JobResultMainWorldSender(pub Sender<JobResult>);

pub(super) fn sync_completed_jobs(
    job_result_receiver: Res<JobResultReceiver>,
    main_job_result_sender: Res<JobResultMainWorldSender>,
    mut commands: Commands,
) {
    while let Ok(job) = job_result_receiver.0.try_recv() {
        main_job_result_sender.0.send(job).unwrap();
        commands.trigger_targets(JobComplete(job.result), job.entity);
        if let Some(mut entity) = commands.get_entity(job.entity) {
            entity.despawn();
        }
    }
}

pub(super) fn sync_completed_jobs_main_world(
    job_result_receiver: Res<JobResultMainWorldReceiver>,
    mut commands: Commands,
) {
    while let Ok(job) = job_result_receiver.0.try_recv() {
        if let Some(main_entity) = job.main_entity {
            commands.trigger_targets(JobComplete(job.result), main_entity.id());
            if let Some(mut entity) = commands.get_entity(main_entity.id()) {
                entity.despawn();
            }
        }
    }
}

pub(super) fn run_jobs(
    jobs: Query<(EntityRef, Option<&MainEntity>, &DynamicJob, &JobPriority), With<JobReady>>,
    world: &World,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    exec_settings: Res<JobExecutionSettings>,
    job_result_sender: Res<JobResultSender>,
    mut command_encoders: Local<Vec<CommandEncoder>>,
) {
    let sorted_jobs = jobs
        .iter()
        .sort::<&JobPriority>()
        .rev()
        .enumerate()
        .take_while(|(i, (_, _, _, priority))| {
            priority.is_critical() || (*i as u32) < exec_settings.max_jobs_per_frame
        })
        .map(|(_, a)| a);

    for (entity_ref, main_entity, job, _) in sorted_jobs {
        let mut command_encoder = render_device.create_command_encoder(&CommandEncoderDescriptor {
            label: Some(job.label().original()),
        });

        let result = job.run(entity_ref, world, &render_device, &mut command_encoder);
        if result.is_ok() {
            command_encoders.push(command_encoder);
        }

        job_result_sender
            .0
            .send(JobResult {
                entity: entity_ref.id(),
                main_entity: main_entity.copied(),
                result,
            })
            .unwrap();
    }

    render_queue.submit(command_encoders.drain(..).map(|cmd| cmd.finish()));
}
