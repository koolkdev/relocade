use super::Post;
use crate::{Error, Result};
use futures_channel::mpsc;
use futures_util::StreamExt;

/// An unbounded, ordered channel between worker tasks.
/// Move each endpoint into its worker through Thread::run before receiving.
pub fn channel<T: Post>() -> Result<(Sender<T>, Receiver<T>)> {
    let (sender, receiver) = mpsc::unbounded();
    Ok((Sender(sender), Receiver(receiver)))
}

/// The sending endpoint of a worker channel. Dropping it ends the stream.
pub struct Sender<T>(mpsc::UnboundedSender<T>);

impl<T: Post> Sender<T> {
    /// Enqueues a message. Success does not acknowledge its receipt.
    pub fn send(&self, value: T) -> Result<()> {
        self.0
            .unbounded_send(value)
            .map_err(|error| Error::new("send worker message", error))
    }
}

/// The receiving endpoint of a worker channel.
pub struct Receiver<T>(mpsc::UnboundedReceiver<T>);

impl<T: Post> Receiver<T> {
    /// Receives the next message, or None after the sender has been dropped.
    pub async fn recv(&mut self) -> Result<Option<T>> {
        Ok(self.0.next().await)
    }
}
