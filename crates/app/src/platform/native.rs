use winit::{
    application::ApplicationHandler,
    error::{EventLoopError, OsError},
    event_loop::{ActiveEventLoop, EventLoop},
    window::{Window, WindowAttributes},
};

pub(super) fn run<T: 'static, A: ApplicationHandler<T> + 'static>(
    event_loop: EventLoop<T>,
    mut application: A,
) -> Result<(), EventLoopError> {
    event_loop.run_app(&mut application)
}

pub(super) fn create_window(
    event_loop: &ActiveEventLoop,
    attributes: WindowAttributes,
) -> Result<Window, OsError> {
    event_loop.create_window(attributes)
}

pub(super) fn detach_window(_window: &Window) {}
