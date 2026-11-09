use super::{Application, Event, HostState, Phase};
use relocade_frontend::mouse::MouseEvent;
use winit::{
    event::{ElementState, MouseButton, WindowEvent},
    event_loop::EventLoopProxy,
    keyboard::{KeyCode, PhysicalKey},
    window::Window,
};

pub(super) struct MouseCapture {
    platform: crate::platform::mouse::MouseCapture,
    capture_click: bool,
    escape: bool,
}

impl MouseCapture {
    pub(super) fn new(window: &Window, events: EventLoopProxy<Event>) -> Result<Self, String> {
        let platform = crate::platform::mouse::MouseCapture::new(window, move || {
            let _ = events.send_event(Event::MouseCaptureChanged);
        })?;
        Ok(Self {
            platform,
            capture_click: false,
            escape: false,
        })
    }

    pub(super) fn captured(&self) -> bool {
        self.platform.captured()
    }

    pub(super) fn release(&mut self, window: &Window) {
        self.platform.release(window);
        self.capture_click = false;
    }

    /// The capture click and Escape belong to the host, including their releases.
    fn window_event(&mut self, window: &Window, host: &mut HostState, event: &WindowEvent) -> bool {
        if let WindowEvent::KeyboardInput { event, .. } = event {
            if event.physical_key == PhysicalKey::Code(KeyCode::Escape)
                && (self.platform.requested() || self.escape)
            {
                let requested = self.platform.requested();
                self.escape = event.state == ElementState::Pressed;
                self.release(window);
                if requested {
                    host.send_mouse(MouseEvent::FocusLost);
                }
                return true;
            }
        }
        if let WindowEvent::MouseInput { button, state, .. } = *event {
            if button == MouseButton::Left && self.capture_click {
                self.capture_click = state == ElementState::Pressed;
            } else if self.captured() {
                host.send_mouse(MouseEvent::Button { button, state });
            } else if button == MouseButton::Left && state == ElementState::Pressed {
                self.capture_click = true;
                self.platform.request(window);
            }
        }
        false
    }
}

impl Application {
    fn mouse_available(&mut self) -> bool {
        let mut host = self.host.borrow_mut();
        if host.mouse.as_ref().is_some_and(|input| input.is_closed()) {
            host.mouse = None;
        }
        host.phase == Phase::Running
            && host.mouse.is_some()
            && self.focused
            && !self.suspended
            && self
                .window
                .as_ref()
                .is_some_and(|window| !window.state().occluded)
    }

    pub(super) fn release_capture(&mut self) {
        if let (Some(capture), Some(window)) = (&mut self.capture, &self.window) {
            capture.release(window.winit());
        }
    }

    pub(super) fn mouse_capture_changed(&mut self) {
        if !self.mouse_available()
            || !self
                .capture
                .as_ref()
                .is_some_and(|capture| capture.captured())
        {
            self.release_capture();
            self.host.borrow_mut().send_mouse(MouseEvent::FocusLost);
        }
    }

    pub(super) fn mouse_window_event(&mut self, event: &WindowEvent) -> bool {
        if !self.mouse_available() {
            self.release_capture();
            return false;
        }
        let Some(capture) = &mut self.capture else {
            return false;
        };
        capture.window_event(
            self.window.as_ref().unwrap().winit(),
            &mut self.host.borrow_mut(),
            event,
        )
    }

    pub(super) fn mouse_motion(&mut self, delta_x: f64, delta_y: f64) {
        if !self.mouse_available() {
            self.release_capture();
        } else if self
            .capture
            .as_ref()
            .is_some_and(|capture| capture.captured())
        {
            self.host
                .borrow_mut()
                .send_mouse(MouseEvent::Motion { delta_x, delta_y });
        }
    }
}
