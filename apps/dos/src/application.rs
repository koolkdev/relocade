use super::{execution, graphics, platform, tasks::Tasks};
use futures_channel::mpsc;
use relocade_app::{
    graphics::{redraw_channel, Redraw},
    winit::{
        application::ApplicationHandler,
        event::WindowEvent,
        event_loop::{ActiveEventLoop, EventLoopProxy},
        window::WindowId,
    },
    worker::Thread,
    Window,
};
use relocade_machine::{timer_service, ExecutionWorker};
use relocade_vga::FrameSource;
use std::{cell::RefCell, rc::Rc};

pub(super) enum Event {
    Poll,
    Frames(FrameSource),
    Redraw,
    DrawComplete { occluded: bool },
    ProgramExited(u8),
    Finished(Result<(), String>),
    Close,
}

pub(super) struct Application {
    events: EventLoopProxy<Event>,
    window: Option<Window>,
    program: Option<Vec<u8>>,
    execution: Option<ExecutionWorker<()>>,
    input: Option<mpsc::UnboundedSender<WindowEvent>>,
    tasks: Tasks,
    redraw: Option<Redraw>,
    redraw_scheduled: bool,
    suspended: bool,
    surface_occluded: bool,
    closing: bool,
    error: Rc<RefCell<Option<String>>>,
}

impl Application {
    pub fn new(
        program: Vec<u8>,
        events: EventLoopProxy<Event>,
        error: Rc<RefCell<Option<String>>>,
    ) -> Self {
        Self {
            tasks: Tasks::new(events.clone()),
            events,
            window: None,
            program: Some(program),
            execution: None,
            input: None,
            redraw: None,
            redraw_scheduled: false,
            suspended: false,
            surface_occluded: false,
            closing: false,
            error,
        }
    }

    fn launch(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
        self.window =
            Some(Window::new(event_loop, platform::attributes()).map_err(|e| e.to_string())?);
        let (clock, timers) = timer_service();
        self.tasks.push(async {
            timers.run().await;
            Event::Finished(Ok(()))
        });
        let program = self.program.take().expect("launch once");
        let (input, incoming) = mpsc::unbounded();
        self.input = Some(input);
        let events = self.events.clone();
        self.execution = Some(ExecutionWorker::spawn(clock, async move |runner| {
            if let Err(error) = execution::run(runner, program, incoming, events.clone()).await {
                let _ = events.send_event(Event::Finished(Err(error)));
            }
        }));
        Ok(())
    }

    fn start_graphics(
        &mut self,
        event_loop: &ActiveEventLoop,
        source: FrameSource,
    ) -> Result<(), String> {
        if self.closing {
            return Ok(());
        }
        let window = self.window.as_ref().expect("window precedes execution");
        let setup = window.surface_setup(event_loop)?;
        let (redraw, requests) = redraw_channel(window.state());
        let thread = Thread::new();
        let events = self.events.clone();
        let task = thread.run_send(setup, move |setup| {
            graphics::run(setup, source, requests, events)
        });
        self.redraw = Some(redraw);
        self.update_output();
        self.tasks.push(async move {
            let result = task
                .await
                .map_err(|e| e.to_string())
                .and_then(|result| result);
            drop(thread); // GPU objects have been released on their owner.
            Event::Finished(result)
        });
        Ok(())
    }

    fn update_output(&mut self) {
        if let (Some(window), Some(redraw)) = (&self.window, &mut self.redraw) {
            let mut output = window.state();
            output.occluded |= self.suspended || self.surface_occluded;
            output.close_requested |= self.closing;
            redraw.output(output);
        }
    }

    fn request_redraw(&mut self) {
        if !self.redraw_scheduled && !self.closing && !self.suspended && !self.surface_occluded {
            if let Some(window) = &self.window {
                if window.state().drawable() {
                    self.redraw_scheduled = true;
                    window.winit().request_redraw();
                }
            }
        }
    }

    fn close(&mut self) {
        self.closing = true;
        self.input = None;
        self.redraw = None; // Finish one outstanding draw, then close the receiver.
        if let Some(execution) = self.execution.take() {
            self.tasks.push(async move {
                Event::Finished(
                    execution
                        .stop()
                        .await
                        .map(|_| ())
                        .map_err(|e| e.to_string()),
                )
            });
        }
    }

    fn fail(&mut self, error: String) {
        if self.error.borrow().is_none() {
            platform::report_error(&error);
            *self.error.borrow_mut() = Some(error);
        }
        self.close();
    }

    fn handle(&mut self, event_loop: &ActiveEventLoop, event: Event) {
        match event {
            Event::Poll => self.tasks.wake_delivered(),
            Event::Frames(source) => {
                if let Err(error) = self.start_graphics(event_loop, source) {
                    self.fail(error);
                }
            }
            Event::Redraw => self.request_redraw(),
            Event::DrawComplete { occluded } if !self.closing => {
                self.surface_occluded = occluded;
                self.update_output();
                if let Some(redraw) = &mut self.redraw {
                    if let Err(error) = redraw.completed() {
                        self.fail(error.to_string());
                    }
                }
            }
            Event::ProgramExited(code) => {
                if let Some(window) = &self.window {
                    window
                        .winit()
                        .set_title(&format!("Relocade DOS — exited {code}"));
                }
            }
            Event::Finished(Err(error)) => self.fail(error),
            Event::Close => self.close(),
            _ => {}
        }
    }

    fn poll(&mut self, event_loop: &ActiveEventLoop) {
        while let Some(event) = self.tasks.poll() {
            self.handle(event_loop, event);
        }
        if self.closing && self.tasks.is_empty() {
            event_loop.exit();
        }
    }
}

impl ApplicationHandler<Event> for Application {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.suspended = false;
        if self.window.is_none() && !self.closing {
            if let Err(error) = self.launch(event_loop) {
                self.fail(error);
            }
        }
        self.surface_occluded = false;
        self.update_output();
        self.request_redraw();
        self.poll(event_loop);
    }

    fn suspended(&mut self, event_loop: &ActiveEventLoop) {
        self.suspended = true;
        self.redraw_scheduled = false;
        self.update_output();
        self.poll(event_loop);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if !self
            .window
            .as_mut()
            .is_some_and(|window| window.handle_event(id, &event))
        {
            return;
        }
        self.update_output();
        match event {
            WindowEvent::CloseRequested | WindowEvent::Destroyed => {
                self.handle(event_loop, Event::Close)
            }
            WindowEvent::KeyboardInput { .. }
            | WindowEvent::ModifiersChanged(_)
            | WindowEvent::Focused(_) => {
                if let Some(input) = &self.input {
                    // Only input events cross to execution; the app keeps window lifecycle.
                    let _ = input.unbounded_send(event);
                }
            }
            WindowEvent::RedrawRequested if !self.closing => {
                self.redraw_scheduled = false;
                self.surface_occluded = false;
                self.update_output();
                if let Some(redraw) = &mut self.redraw {
                    if let Err(error) = redraw.request() {
                        self.fail(error.to_string());
                    }
                }
            }
            WindowEvent::Resized(_)
            | WindowEvent::ScaleFactorChanged { .. }
            | WindowEvent::Occluded(false) => {
                self.surface_occluded = false;
                self.update_output();
                self.request_redraw();
            }
            _ => {}
        }
        self.poll(event_loop);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: Event) {
        self.handle(event_loop, event);
        self.poll(event_loop);
    }

    fn exiting(&mut self, _: &ActiveEventLoop) {
        self.window = None;
    }
}
