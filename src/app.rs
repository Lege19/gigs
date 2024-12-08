use bevy_app::App;

use super::{GraphicsJob, SpecializedGraphicsJobPlugin};

pub trait InitGraphicsJobExt {
    fn init_graphics_job<J: GraphicsJob>(&mut self) -> &mut Self;
}

impl InitGraphicsJobExt for App {
    fn init_graphics_job<J: GraphicsJob>(&mut self) -> &mut Self {
        self.add_plugins(SpecializedGraphicsJobPlugin::<J>::default())
    }
}
