use bevy::prelude::*;
use bevy_render::{render_resource::CommandEncoder, renderer::RenderDevice};
use gigs::{
    GraphicsJob, GraphicsJobsPlugin, InitGraphicsJobExt, JobComplete, JobError, JobInputItem,
};

fn main() -> AppExit {
    let mut app = App::new();

    app.add_plugins(DefaultPlugins)
        .add_plugins(GraphicsJobsPlugin::default())
        .init_graphics_job::<BasicJob>();

    app.world_mut()
        .spawn(BasicJob)
        .observe(|_trigger: Trigger<JobComplete>| println!("Job done!"));

    app.run()
}

#[derive(Clone, Component)]
struct BasicJob;

impl GraphicsJob for BasicJob {
    type In = ();

    fn run(
        &self,
        _world: &World,
        _render_device: &RenderDevice,
        _command_encoder: &mut CommandEncoder,
        (): JobInputItem<Self, Self::In>,
    ) -> Result<(), JobError> {
        println!("Job running!");
        Ok(())
    }
}
