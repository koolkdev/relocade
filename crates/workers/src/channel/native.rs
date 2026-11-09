use crate::{Post, ResourceError};
use futures_channel::mpsc;
use futures_util::StreamExt;
use std::task::{Context, Poll};

pub struct Sender<P: Post>(mpsc::UnboundedSender<P>);
pub struct ReceiverEndpoint<P: Post>(mpsc::UnboundedReceiver<P>);
pub struct Receiver<P: Post>(mpsc::UnboundedReceiver<P>);

pub fn channel<P: Post>() -> Result<(Sender<P>, ReceiverEndpoint<P>), ResourceError> {
    let (sender, receiver) = mpsc::unbounded();
    Ok((Sender(sender), ReceiverEndpoint(receiver)))
}

impl<P: Post> Sender<P> {
    pub fn send(&self, posted: P) -> Result<(), ResourceError> {
        self.0
            .unbounded_send(posted)
            .map_err(|_| ResourceError::Closed)
    }
}

impl<P: Post> ReceiverEndpoint<P> {
    pub fn bind(self) -> Receiver<P> {
        Receiver(self.0)
    }
}

impl<P: Post> Receiver<P> {
    pub fn poll_recv(&mut self, cx: &mut Context<'_>) -> Poll<Result<P, ResourceError>> {
        self.0
            .poll_next_unpin(cx)
            .map(|posted| posted.ok_or(ResourceError::Closed))
    }
}
