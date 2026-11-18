use futures_channel::oneshot;
use futures_lite::{future, io::AsyncReadExt};
use relocade_machine::Runner;
use relocade_parallel::centronics::{Input, Status};
use std::{io, num::NonZeroUsize};

enum Request {
    Snapshot(oneshot::Sender<Vec<u8>>),
    RemainingCapacity(oneshot::Sender<usize>),
    Clear(oneshot::Sender<()>),
}

enum Event {
    Request(Result<Request, async_channel::RecvError>),
    Input(bool),
}

/// Bounded raw output, retained until explicitly cleared.
///
/// The session activity owns the reader and byte ordering. Snapshots include
/// the connection's already accepted byte and leave the original data intact.
/// Filling the buffer asserts BUSY; clearing it wakes pending writers. INIT,
/// AUTOFEED, form feeds and writer closure never discard captured data.
/// Dropping the handle detaches the peripheral. Session shutdown ends capture.
///
/// This handle may travel to the execution worker's host. Additional workers
/// must bind the runner's wake peer before sending requests, as for other
/// channels that notify execution.
pub struct Capture {
    requests: async_channel::Sender<Request>,
}

impl Capture {
    pub async fn attach(runner: &Runner, input: Input, capacity: NonZeroUsize) -> Self {
        let (requests, incoming) = async_channel::bounded(1);
        runner
            .spawn_activity(
                Buffer {
                    input,
                    bytes: Vec::new(),
                    capacity: capacity.get(),
                }
                .run(incoming),
            )
            .await;
        Self { requests }
    }

    /// Copies the raw job at the time execution handles this request.
    /// Cancelling this operation cannot consume or discard the job.
    pub async fn snapshot(&self) -> io::Result<Vec<u8>> {
        self.request(Request::Snapshot).await
    }

    /// Returns the number of free bytes after accounting for accepted input.
    pub async fn remaining_capacity(&self) -> io::Result<usize> {
        self.request(Request::RemainingCapacity).await
    }

    /// Discards the current job, including the connection's pending byte.
    /// Once submitted, a clear still takes effect if this future is cancelled.
    pub async fn clear(&self) -> io::Result<()> {
        self.request(Request::Clear).await
    }

    async fn request<T>(
        &self,
        request: impl FnOnce(oneshot::Sender<T>) -> Request,
    ) -> io::Result<T> {
        let (reply, received) = oneshot::channel();
        self.requests
            .send(request(reply))
            .await
            .map_err(|_| disconnected())?;
        received.await.map_err(|_| disconnected())
    }
}

fn disconnected() -> io::Error {
    io::Error::new(io::ErrorKind::BrokenPipe, "Printer capture has ended")
}

struct Buffer {
    input: Input,
    bytes: Vec<u8>,
    capacity: usize,
}

impl Buffer {
    async fn run(mut self, requests: async_channel::Receiver<Request>) {
        loop {
            let request = if self.bytes.len() == self.capacity {
                requests.recv().await
            } else {
                // Commands have priority over new input. Snapshot and Clear
                // drain the one accepted byte before establishing their boundary.
                match future::or(async { Event::Request(requests.recv().await) }, async {
                    Event::Input(self.read().await)
                })
                .await
                {
                    Event::Request(request) => request,
                    Event::Input(true) => continue,
                    Event::Input(false) => return,
                }
            };
            let Ok(request) = request else { return };
            if self.bytes.len() < self.capacity {
                let _ = future::poll_once(self.read()).await;
            }
            match request {
                Request::Snapshot(reply) => {
                    if !reply.is_canceled() {
                        let _ = reply.send(self.bytes.clone());
                    }
                }
                Request::RemainingCapacity(reply) => {
                    let _ = reply.send(self.capacity - self.bytes.len());
                }
                Request::Clear(reply) => {
                    self.bytes.clear();
                    self.publish_capacity();
                    let _ = reply.send(());
                }
            }
        }
    }

    async fn read(&mut self) -> bool {
        let mut byte = [0];
        match self.input.read(&mut byte).await {
            Ok(1) => {
                self.bytes.push(byte[0]);
                // Publish before yielding: no additional byte can be accepted
                // after the final free slot has been consumed.
                self.publish_capacity();
                true
            }
            _ => false,
        }
    }

    fn publish_capacity(&self) {
        self.input.set_status(Status {
            ready: self.bytes.len() < self.capacity,
            ..Status::default()
        });
    }
}
