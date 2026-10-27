use crate::{
    application, display, platform::RenderTarget, Application, Exit, OnCompletion, Renderer, Task,
};
use futures_channel::oneshot;
use std::{future::Future, mem, pin::Pin, task::Context, task::Poll};

/// Owns application execution, both worker lifetimes and the completion policy.
pub(crate) struct Session<A: Application, R: Renderer> {
    execution: Execution<A>,
    graphics: Option<display::Display<R::Source>>,
    on_completion: OnCompletion,
    closing: bool,
    error: Option<String>,
    finished: Option<oneshot::Sender<Result<Exit<A::Output>, String>>>,
}

enum Execution<A: Application> {
    Waiting(A),
    Running(Task<A::Output>),
    Finished(Exit<A::Output>),
}

impl<A: Application<Update = R::Source>, R: Renderer> Session<A, R> {
    pub fn new(
        application: A,
        on_completion: OnCompletion,
        finished: oneshot::Sender<Result<Exit<A::Output>, String>>,
    ) -> Self {
        Self {
            execution: Execution::Waiting(application),
            graphics: None,
            on_completion,
            closing: false,
            error: None,
            finished: Some(finished),
        }
    }

    pub fn start(
        &mut self,
        target: RenderTarget,
        application_update: impl Fn(A::Update) + Send + 'static,
        frame_done: impl Fn() + Send + 'static,
    ) {
        let Execution::Waiting(application) =
            mem::replace(&mut self.execution, Execution::Finished(Exit::Cancelled))
        else {
            unreachable!("a session starts only once");
        };
        self.graphics = Some(display::Display::new::<R>(target, frame_done));
        self.execution = Execution::Running(application::spawn(application, application_update));
    }

    pub fn is_closing(&self) -> bool {
        self.closing
    }

    pub fn is_finished(&self) -> bool {
        self.closing && !matches!(self.execution, Execution::Running(_)) && self.graphics.is_none()
    }

    pub fn fail(&mut self, error: String) {
        self.error.get_or_insert(error);
        self.stop();
    }

    pub fn update(&self, source: R::Source) {
        // Execution can complete before its final source update arrives. Keep
        // accepting it while KeepOpen retains the session's host window.
        self.render(display::Command::Source(source));
    }

    pub fn render(&self, command: display::Command<R::Source>) {
        if !self.closing {
            if let Some(graphics) = &self.graphics {
                graphics.send(command);
            }
        }
    }

    pub fn poll(&mut self, context: &mut Context<'_>) {
        if let Execution::Running(task) = &mut self.execution {
            if let Poll::Ready(result) = Pin::new(task).poll(context) {
                match result {
                    Ok(result) => {
                        self.execution = Execution::Finished(result);
                        if matches!(self.on_completion, OnCompletion::Close) {
                            self.stop();
                        }
                    }
                    Err(error) => {
                        self.execution = Execution::Finished(Exit::Cancelled);
                        self.fail(error);
                    }
                }
            }
        }
        if let Some(graphics) = &mut self.graphics {
            if let Poll::Ready(result) = Pin::new(graphics).poll(context) {
                self.graphics = None;
                match result {
                    Ok(()) if self.closing => {}
                    Ok(()) => self.fail("The graphics worker stopped unexpectedly".into()),
                    Err(error) => self.fail(error),
                }
            }
        }
    }

    /// Called after the window detaches. A forced event-loop exit cannot report
    /// successful completion while either accepted worker task is still alive.
    pub fn finish(&mut self) {
        self.stop();
        if !self.is_finished() {
            self.error.get_or_insert_with(|| {
                "The window event loop ended before its workers finished".into()
            });
        }
        if let Some(finished) = self.finished.take() {
            let result = if let Some(error) = self.error.take() {
                Err(error)
            } else {
                let Execution::Finished(result) =
                    mem::replace(&mut self.execution, Execution::Finished(Exit::Cancelled))
                else {
                    unreachable!("finished session has no active execution task");
                };
                Ok(result)
            };
            let _ = finished.send(result);
        }
    }
}

impl<A: Application, R: Renderer> Session<A, R> {
    pub fn stop(&mut self) {
        self.closing = true;
        match &self.execution {
            Execution::Waiting(_) => self.execution = Execution::Finished(Exit::Cancelled),
            Execution::Running(task) => task.stop(),
            Execution::Finished(_) => {}
        }
        if let Some(graphics) = &self.graphics {
            graphics.stop();
        }
    }
}

impl<A: Application, R: Renderer> Drop for Session<A, R> {
    fn drop(&mut self) {
        self.stop();
    }
}
