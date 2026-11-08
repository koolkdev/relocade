use super::{keyboard, Application, Event, Phase};
use crate::Window;
use relocade_frontend::mouse::MouseEvent;
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::{DeviceEvent, DeviceId, WindowEvent},
    event_loop::ActiveEventLoop,
    window::WindowId,
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
        self.focused = self
            .window
            .as_ref()
            .is_some_and(|window| window.winit().has_focus());
        self.request_redraw();
        self.poll(event_loop);
    }

    fn suspended(&mut self, _: &ActiveEventLoop) {
        self.suspended = true;
        self.focused = false;
        self.host.borrow_mut().release_input();
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
        if let WindowEvent::Focused(focused) = event {
            self.focused = focused;
        }
        if self.host.borrow().phase == Phase::Running && !state.close_requested {
            if matches!(
                event,
                WindowEvent::Focused(false) | WindowEvent::Occluded(true)
            ) {
                self.host.borrow_mut().release_input();
            }
            #[cfg(feature = "audio")]
            crate::platform::application::window_event(&mut self.host.borrow_mut().audio);
            if let Some(event) = keyboard::translate(&event) {
                self.host.borrow_mut().send_key(event);
            }
            if self.focused && !self.suspended && !state.occluded {
                if let WindowEvent::MouseInput { button, state, .. } = event {
                    self.host
                        .borrow_mut()
                        .send_mouse(MouseEvent::Button { button, state });
                }
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

    fn device_event(&mut self, _: &ActiveEventLoop, _: DeviceId, event: DeviceEvent) {
        if self.focused
            && !self.suspended
            && self
                .window
                .as_ref()
                .is_some_and(|window| !window.state().occluded)
            && self.host.borrow().phase == Phase::Running
        {
            if let DeviceEvent::MouseMotion {
                delta: (delta_x, delta_y),
            } = event
            {
                self.host
                    .borrow_mut()
                    .send_mouse(MouseEvent::Motion { delta_x, delta_y });
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
