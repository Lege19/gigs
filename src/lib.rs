//! `gigs`: on-demand graphics jobs for `bevy`
//!
//! Gigs is a plugin for the Bevy game engine that aims to provide a simple
//! abstraction for "graphics jobs", units of rendering work that only need to be
//! done sporadically, on-demand. For example, a terrain generation compute shader
//! would only need to be run once for each chunk of terrain. In many cases, this
//! crate will allow you to skip most or all of the manual extraction and resource
//! prep boilerplate that comes along with this, and focus on writing shaders.
//!
//! Getting started:
//!
//! 1. First, add `gigs` to your Cargo dependencies: `cargo add gigs`
//! 2. Add `GraphicsJobsPlugin` to your `App`
//! 3. Implement `GraphicsJob` for your job component
//! 4. Call `init_graphics_job` on `App` to initialize your custom job
//! 5. To run the job, simply spawn an entity with your job component!
//!
//! See the examples in the repo for more in-depth showcases!

#![allow(clippy::type_complexity)]

mod ext;
pub mod input;
pub mod meta;
mod runner;
use disqualified::ShortName;
pub use ext::*;
use input::{JobInput, JobInputItem};
use meta::{extract_job_meta, JobMarker};
use runner::{
    check_job_inputs, erase_jobs, increment_time_out_frames, run_jobs, setup_time_out_frames,
    sync_completed_jobs, sync_completed_jobs_main_world, time_out_jobs, JobResultMainWorldReceiver,
    JobResultMainWorldSender, JobResultReceiver, JobResultSender, JobSet,
};

use core::marker::PhantomData;

use bevy_app::{App, Plugin, Update};
use bevy_ecs::{
    component::Component,
    event::Event,
    query::Added,
    schedule::{IntoSystemConfigs, IntoSystemSetConfigs},
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

/// A trait for components describing a unit of rendering work.
///
/// When a [`Component`] implementing this trait is added to the [`World`],
/// it is extracted to the render world, where it waits for its inputs to be
/// prepared. When they are ready, it will execute and the commands it encodes
/// will be submitted before the render graph is executed.
///
/// You can also specify a priority for a running job by adding the [`JobPriority`](meta::JobPriority)
/// component when it is spawned.
///
/// Note: you must call [`init_graphics_job`](crate::ext::InitGraphicsJobExt::init_graphics_job)
/// on [`App`] for the job to execute.
pub trait GraphicsJob: Component + Clone {
    type In: JobInput<Self>;

    fn label() -> ShortName<'static> {
        ShortName::of::<Self>()
    }

    fn run(
        &self,
        world: &World,
        render_device: &RenderDevice,
        command_encoder: &mut CommandEncoder,
        input: JobInputItem<Self, Self::In>,
    ) -> Result<(), JobError>;
}

/// The main plugin for `gigs`. This plugin is needed for all functionality.
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

        let (main_sender, main_receiver) = crossbeam_channel::unbounded();

        app.insert_resource(JobResultMainWorldReceiver(main_receiver))
            .add_systems(Update, sync_completed_jobs_main_world);

        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            let (sender, receiver) = crossbeam_channel::unbounded();
            render_app
                .insert_resource(JobResultSender(sender))
                .insert_resource(JobResultReceiver(receiver))
                .insert_resource(JobResultMainWorldSender(main_sender));

            render_app.add_systems(ExtractSchedule, extract_job_meta);

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
                    setup_time_out_frames.in_set(JobSet::Setup),
                    check_job_inputs.in_set(JobSet::Check),
                    time_out_jobs.in_set(JobSet::Check),
                    run_jobs.in_set(JobSet::Execute),
                    increment_time_out_frames.in_set(JobSet::Cleanup),
                    sync_completed_jobs.in_set(JobSet::Cleanup),
                ),
            );
        }
    }
}

/// Settings for how jobs are scheduled each frame
#[derive(Copy, Clone, Resource, ExtractResource)]
pub struct JobExecutionSettings {
    /// The maximum number of jobs to execute each frame. This number
    /// may be exceeded in the case that a large number of jobs are
    /// queued with [`Priority::Critical`](meta::Priority::Critical).
    pub max_jobs_per_frame: u32,
    /// The maximum number of frames a job should wait to execute
    /// before timing out.
    pub time_out_frames: u32,
}

impl Default for JobExecutionSettings {
    fn default() -> Self {
        Self {
            max_jobs_per_frame: 16,
            time_out_frames: 16,
        }
    }
}

/// A plugin that sets up logic for a specific implementation of [`GraphicsJob`].
/// It's recommended to call [`init_graphics_job`](crate::ext::InitGraphicsJobExt::init_graphics_job)
/// on [`App`] rather than add this plugin manually.
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

/// An event signaling a completed (or failed) graphics job.
#[derive(Event, Copy, Clone, Debug)]
pub struct JobComplete(pub Result<(), JobError>);

/// Describes how an incomplete job may have failed.
#[derive(Copy, Clone, Debug)]
pub enum JobError {
    /// Signals a job that failed due to timing out, either
    /// because its needed resources were not ready in time,
    /// or because too many jobs were scheduled ahead of it.
    TimedOut,
    /// Signals a job that failed because its inputs were
    /// unable to be satisfied, for example if a needed
    /// extra component was not provided by the user.
    InputsFailed,
    /// Signals a job that failed during execution.
    ExecutionFailed,
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
