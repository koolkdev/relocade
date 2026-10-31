use super::{keyboard, Application, Event, Phase};
use crate::Window;
use relocade_frontend::keyboard::KeyboardEvent;
use winit::{
    application::ApplicationHandler, dpi::PhysicalSize, event::WindowEvent,
    event_loop::ActiveEventLoop, window::WindowId,
};

impl ApplicationHandler<Event> for Application {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.suspended = false;
        if let Some(attributes) = self.attributes.take() {
            match Window::new(event_loop, attributes) {
                Ok(window) => self.window = Some(window),
                Err(error) => self.close(Err(error.to_string())),
            }
        }
        self.request_redraw();
        self.poll(event_loop);
    }

    fn suspended(&mut self, _: &ActiveEventLoop) {
        self.suspended = true;
        self.host.borrow_mut().send_key(KeyboardEvent::FocusLost);
        if let Some(graphics) = &mut self.graphics {
            graphics.request_redraw(PhysicalSize::new(0, 0));
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(window) = &mut self.window else {
            return;
        };
        if !window.handle_event(id, &event) {
            return;
        }
        let state = window.state();
        if self.host.borrow().phase == Phase::Running {
            if let Some(event) = keyboard::translate(&event) {
                self.host.borrow_mut().send_key(event);
            }
        }
        if state.close_requested {
            self.close(Ok(()));
            self.poll(event_loop);
        } else if !self.suspended && self.host.borrow().phase == Phase::Running {
            match event {
                WindowEvent::RedrawRequested => {
                    if state.drawable() {
                        if let Some(graphics) = &mut self.graphics {
                            graphics.request_redraw(state.size);
                        }
                    }
                }
                WindowEvent::Resized(_)
                | WindowEvent::ScaleFactorChanged { .. }
                | WindowEvent::Occluded(_) => {
                    if !state.drawable() {
                        if let Some(graphics) = &mut self.graphics {
                            graphics.request_redraw(PhysicalSize::new(0, 0));
                        }
                    }
                    self.request_redraw();
                }
                _ => {}
            }
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: Event) {
        match event {
            Event::Wake => {
                self.wake.clear();
                self.poll(event_loop);
            }
            Event::RedrawNeeded => self.request_redraw(),
            Event::Close => {
                self.close(Ok(()));
                self.poll(event_loop);
            }
        }
    }
}

impl Application {
    fn request_redraw(&self) {
        if self.suspended || self.host.borrow().phase != Phase::Running {
            return;
        }
        if let Some(window) = &self.window {
            if window.state().drawable() {
                window.winit().request_redraw();
            }
        }
    }
}
