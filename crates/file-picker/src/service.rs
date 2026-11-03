use crate::{platform, Access, File};
use futures_channel::mpsc;
use futures_util::{
    future::{AbortHandle, AbortRegistration, Abortable, LocalBoxFuture},
    StreamExt,
};
use std::{cell::RefCell, io};

mod resource;
pub(crate) use resource::{Resource, Resources};

/// Main-thread owner of platform resources and their pending I/O.
///
/// Pick on this owner, then send the returned Rust handles to other workers.
/// Poll `run()` concurrently with their operations. Each resource processes its
/// requests in order; unrelated resources can make progress independently.
/// No application worker or event loop is created here.
pub struct FileService {
    resources: Resources,
    pending: RefCell<
        Option<(
            mpsc::UnboundedReceiver<LocalBoxFuture<'static, ()>>,
            AbortRegistration,
        )>,
    >,
    stop: AbortHandle,
}

impl Default for FileService {
    fn default() -> Self {
        Self::new()
    }
}

impl FileService {
    pub fn new() -> Self {
        let (tasks, pending) = mpsc::unbounded();
        let (stop, registration) = AbortHandle::new_pair();
        Self {
            resources: Resources { tasks },
            pending: RefCell::new(Some((pending, registration))),
            stop,
        }
    }

    /// Selects a read-only file. `rfd` reports both cancellation and some dialog
    /// failures as no selection. Reads themselves return ordinary I/O errors.
    pub async fn pick_file(&self) -> io::Result<Option<File>> {
        self.ensure_open()?;
        match platform::pick_file().await? {
            Some((name, file)) => self
                .resources
                .register_file(name, Access::ReadOnly, file)
                .map(Some),
            None => Ok(None),
        }
    }

    /// Drives resource requests on the service's thread until closed.
    /// Poll alongside the application. Cancelling this future also closes all
    /// resource channels. A completed or cancelled service cannot be restarted.
    pub async fn run(&self) {
        let Some((pending, registration)) = self.pending.borrow_mut().take() else {
            return;
        };
        let _ = Abortable::new(
            pending.for_each_concurrent(None, |activity| activity),
            registration,
        )
        .await;
    }

    /// Wakes `run()` to release resources and reject queued/in-flight requests.
    /// Await `run()` finishing for cleanup. Already-started host I/O may finish;
    /// closure is not a rollback of filesystem changes.
    pub fn close(&self) {
        self.resources.tasks.close_channel();
        self.stop.abort();
        self.pending.borrow_mut().take();
    }

    fn ensure_open(&self) -> io::Result<()> {
        if self.resources.tasks.is_closed() {
            Err(closed())
        } else {
            Ok(())
        }
    }
}

impl Drop for FileService {
    fn drop(&mut self) {
        self.close();
    }
}

pub(crate) fn closed() -> io::Error {
    io::Error::new(io::ErrorKind::BrokenPipe, "file service is closed")
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
