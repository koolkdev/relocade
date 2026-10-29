use super::{graphics, host::Event, platform::SurfaceSetup, Input, WindowApplication};
use crate::{spawn, worker::Worker, Task};
use arc_swap::ArcSwapOption;
use futures_channel::{mpsc, oneshot};
use relocade_machine::{timer_service, TimerDriver};
use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};
use winit::event_loop::EventLoopProxy;

pub(crate) struct Session<A: WindowApplication> {
    execution: Option<Task<A>>,
    graphics: Option<Worker<Result<(), String>>>,
    timers: Option<Pin<Box<dyn Future<Output = ()>>>>,
    source: Arc<ArcSwapOption<A::Source>>,
    requests: Option<mpsc::UnboundedSender<[u32; 2]>>,
    result: Option<A::Output>,
    error: Option<String>,
    closing: bool,
    finished: Option<oneshot::Sender<Result<Option<A::Output>, String>>>,
}
impl<A: WindowApplication> Session<A> {
    pub fn new(
        application: A,
        finished: oneshot::Sender<Result<Option<A::Output>, String>>,
    ) -> Self {
        let (clock, timers) = timer_service();
        let execution = spawn(application, clock);
        Self {
            source: execution.source.clone(),
            execution: Some(execution),
            graphics: None,
            timers: Some(Box::pin(TimerDriver::run(timers))),
            requests: None,
            result: None,
            error: None,
            closing: false,
            finished: Some(finished),
        }
    }
    pub fn start_graphics(&mut self, setup: SurfaceSetup, events: EventLoopProxy<Event>) {
        let (requests, incoming) = mpsc::unbounded();
        let source = self.source.clone();
        self.graphics = Some(Worker::spawn(setup, move |setup| {
            graphics::run::<A>(setup, source, incoming, events)
        }));
        self.requests = Some(requests);
    }
    pub fn input(&self, input: Input) {
        if let Some(execution) = &self.execution {
            execution.send_input(input);
        }
    }
    pub fn redraw(&self, size: [u32; 2]) {
        if let Some(requests) = &self.requests {
            let _ = requests.unbounded_send(size);
        }
    }
    pub fn closing(&self) -> bool {
        self.closing
    }
    pub fn stop(&mut self) {
        self.closing = true;
        self.requests = None;
        if let Some(execution) = &self.execution {
            execution.stop();
        }
        if let Some(graphics) = &self.graphics {
            graphics.stop();
        }
    }
    pub fn fail(&mut self, error: String) {
        self.error.get_or_insert(error);
        self.stop();
    }
    pub fn poll(&mut self, cx: &mut Context<'_>) -> bool {
        if let Some(execution) = &mut self.execution {
            if let Poll::Ready(result) = Pin::new(execution).poll(cx) {
                self.execution = None;
                match result {
                    Ok(output) => self.result = output,
                    Err(error) => self.fail(error),
                }
            }
        }
        if let Some(graphics) = &mut self.graphics {
            if let Poll::Ready(result) = graphics.poll(cx) {
                self.graphics = None;
                match result {
                    Ok(Some(Err(error))) | Err(error) => self.fail(error),
                    _ => self.stop(),
                }
            }
        }
        if let Some(timers) = &mut self.timers {
            if timers.as_mut().poll(cx).is_ready() {
                self.timers = None;
            }
        }
        if self.closing && self.execution.is_none() && self.graphics.is_none() {
            // Sources/results may retain timer handles; cleanup ends this host's
            // timer lifetime without waiting for those values to be discarded.
            self.timers = None;
            true
        } else {
            false
        }
    }
    pub fn finish(&mut self) {
        if let Some(finished) = self.finished.take() {
            let result = self
                .error
                .take()
                .map_or_else(|| Ok(self.result.take()), Err);
            let _ = finished.send(result);
        }
    }
}
