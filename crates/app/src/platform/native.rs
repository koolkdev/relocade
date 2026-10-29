use winit::{application::ApplicationHandler, error::EventLoopError, event_loop::EventLoop};

pub(super) fn run<T: 'static, A: ApplicationHandler<T> + 'static>(
    event_loop: EventLoop<T>,
    mut application: A,
) -> Result<(), EventLoopError> {
    event_loop.run_app(&mut application)
}
