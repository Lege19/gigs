#![allow(clippy::type_complexity)]

mod app;
mod input;
mod meta;
mod runner;
pub use app::*;
use disqualified::ShortName;
pub use input::*;
pub use meta::*;
use runner::{
    cancel_stalled_jobs, check_job_inputs, erase_jobs, increment_frames_stalled, run_jobs,
    setup_job_misc_components, sync_completed_jobs,
};

use core::marker::PhantomData;

use bevy_app::{App, Plugin};
use bevy_ecs::{
    component::Component,
    event::Event,
    query::Added,
    schedule::{IntoSystemConfigs, IntoSystemSetConfigs, SystemSet},
    system::{Commands, Query, Resource},
    world::World,
};
use bevy_render::{
    extract_resource::{ExtractResource, ExtractResourcePlugin},
    render_resource::CommandEncoder,
    renderer::RenderDevice,
    sync_component::SyncComponentPlugin,
    ExtractSchedule, Render, RenderApp, RenderSet,
};
use bevy_render::{sync_world::RenderEntity, Extract};

#[derive(Default)]
pub struct GraphicsJobsPlugin {
    settings: JobExecutionSettings,
}

impl Plugin for GraphicsJobsPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.settings);

        app.add_plugins((
            SyncComponentPlugin::<JobMarker>::default(),
            ExtractResourcePlugin::<JobExecutionSettings>::default(),
        ));

        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            render_app.add_systems(ExtractSchedule, (extract_job_meta, sync_completed_jobs));

            render_app.configure_sets(
                Render,
                (
                    JobSet::Setup,
                    JobSet::Check,
                    JobSet::Execute,
                    JobSet::Cleanup,
                )
                    .chain(),
            );

            render_app.configure_sets(
                Render,
                (
                    JobSet::Check.after(RenderSet::Prepare),
                    JobSet::Execute.before(RenderSet::Render),
                    JobSet::Cleanup.in_set(RenderSet::Cleanup),
                ),
            );

            render_app.add_systems(
                Render,
                (
                    setup_job_misc_components.in_set(JobSet::Setup),
                    check_job_inputs.in_set(JobSet::Check),
                    cancel_stalled_jobs.in_set(JobSet::Check),
                    increment_frames_stalled.in_set(JobSet::Cleanup),
                    run_jobs.in_set(JobSet::Execute),
                ),
            );
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, SystemSet)]
pub enum JobSet {
    Setup,
    Check,
    Execute,
    Cleanup,
}

#[derive(Copy, Clone, Resource, ExtractResource)]
pub struct JobExecutionSettings {
    pub max_jobs_per_frame: u32,
    pub max_job_stall_frames: u32,
}

impl Default for JobExecutionSettings {
    fn default() -> Self {
        Self {
            max_jobs_per_frame: 16,
            max_job_stall_frames: 16,
        }
    }
}

pub struct SpecializedGraphicsJobPlugin<J: GraphicsJob>(PhantomData<J>);

impl<J: GraphicsJob> Default for SpecializedGraphicsJobPlugin<J> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

impl<J: GraphicsJob> Plugin for SpecializedGraphicsJobPlugin<J> {
    fn build(&self, app: &mut App) {
        app.add_plugins(<J as GraphicsJob>::In::plugin());

        app.register_required_components::<J, JobMarker>();

        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            render_app
                .add_systems(ExtractSchedule, extract_jobs::<J>)
                .add_systems(Render, erase_jobs::<J>.in_set(JobSet::Setup));
        }
    }
}

#[derive(Event, Copy, Clone)]
pub struct JobComplete(pub Result<(), JobError>);

#[derive(Copy, Clone)]
pub enum JobError {
    Stalled,
    InputsNotSatisfied,
    CommandEncodingFailed,
}

pub trait GraphicsJob: Component + Clone {
    type In: JobInput<Self>;

    fn label() -> ShortName<'static> {
        ShortName::of::<Self>()
    }

    fn run(
        &self,
        world: &World,
        render_device: &RenderDevice,
        commands: &mut CommandEncoder,
        input: JobInputItem<Self, Self::In>,
    ) -> Result<(), JobError>;
}

fn extract_jobs<J: GraphicsJob>(
    jobs: Extract<Query<(RenderEntity, &J), Added<JobMarker>>>,
    mut commands: Commands,
) {
    let cloned_jobs = jobs
        .iter()
        .map(|(entity, job)| (entity, job.clone()))
        .collect::<Vec<_>>();
    commands.insert_batch(cloned_jobs);
}

#[derive(Clone, PartialEq, Eq, Debug, Component)]
enum JobStatus {
    Waiting,
    Ready,
    Done,
}
