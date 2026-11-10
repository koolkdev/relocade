use super::ChannelError;
use crate::Post;
use futures_channel::mpsc;
use futures_util::StreamExt;
use std::task::{Context, Poll};

pub struct Sender<P: Post>(mpsc::UnboundedSender<P>);
pub struct Receiver<P: Post>(mpsc::UnboundedReceiver<P>);

pub fn channel<P: Post>() -> Result<(Sender<P>, Receiver<P>), ChannelError> {
    let (sender, receiver) = mpsc::unbounded();
    Ok((Sender(sender), Receiver(receiver)))
}

impl<P: Post> Sender<P> {
    pub fn send(&self, payload: P) -> Result<(), ChannelError> {
        self.0
            .unbounded_send(payload)
            .map_err(|_| ChannelError::Closed)
    }
}

impl<P: Post> Receiver<P> {
    pub fn poll_recv(&mut self, cx: &mut Context<'_>) -> Poll<Result<P, ChannelError>> {
        self.0
            .poll_next_unpin(cx)
            .map(|payload| payload.ok_or(ChannelError::Closed))
    }
}
