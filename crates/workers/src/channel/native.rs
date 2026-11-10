use super::ChannelError;
use crate::Post;
use futures_channel::mpsc;
use futures_util::StreamExt;
use std::task::{Context, Poll};

pub struct Sender<P: Post>(mpsc::UnboundedSender<P>);
pub struct ReceiverEndpoint<P: Post>(mpsc::UnboundedReceiver<P>);
pub struct Receiver<P: Post>(mpsc::UnboundedReceiver<P>);

pub fn channel<P: Post>() -> Result<(Sender<P>, ReceiverEndpoint<P>), ChannelError> {
    let (sender, receiver) = mpsc::unbounded();
    Ok((Sender(sender), ReceiverEndpoint(receiver)))
}

impl<P: Post> ReceiverEndpoint<P> {
    pub(super) fn bind(self) -> Receiver<P> {
        Receiver(self.0)
    }
}

impl<P: Post> Sender<P> {
    pub(super) fn send(&self, payload: P) -> Result<(), ChannelError> {
        self.0
            .unbounded_send(payload)
            .map_err(|_| ChannelError::Closed)
    }
}

impl<P: Post> Receiver<P> {
    pub(super) fn poll_recv(&mut self, cx: &mut Context<'_>) -> Poll<Result<P, ChannelError>> {
        self.0
            .poll_next_unpin(cx)
            .map(|payload| payload.ok_or(ChannelError::Closed))
    }
}
