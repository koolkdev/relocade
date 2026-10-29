use winit::{
    application::ApplicationHandler, error::EventLoopError, event_loop::EventLoop,
    platform::web::EventLoopExtWebSys,
};

pub(super) fn run<T: 'static, A: ApplicationHandler<T> + 'static>(
    event_loop: EventLoop<T>,
    application: A,
) -> Result<(), EventLoopError> {
    event_loop.spawn_app(application);
    Ok(())
}
