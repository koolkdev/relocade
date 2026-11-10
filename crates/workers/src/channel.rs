//! Typed messages with Rust state and posted objects delivered in the same order.

use crate::{SendTask, Thread, Transfer};
use futures_channel::mpsc;
use futures_util::StreamExt;
use std::{
    fmt,
    future::Future,
    task::{ready, Context, Poll},
};

#[cfg_attr(not(target_arch = "wasm32"), path = "channel/native.rs")]
#[cfg_attr(target_arch = "wasm32", path = "channel/web.rs")]
mod platform;

/// Failure to send or reconstruct a worker message.
#[derive(Clone, Debug)]
pub enum ChannelError {
    Closed,
    Transport(String),
}

impl fmt::Display for ChannelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Closed => f.write_str("worker channel closed"),
            Self::Transport(error) => f.write_str(error),
        }
    }
}

impl std::error::Error for ChannelError {}

/// The single producer for an ordered worker channel.
/// Transfer this endpoint to the producer, including through another channel.
pub struct Sender<T: Transfer> {
    payloads: platform::Sender<T::Payload>,
    states: mpsc::UnboundedSender<T::State>,
}

/// Receives messages on its owning thread: the caller for [`channel`], or the
/// worker for [`crate::Worker::listen`].
/// Browser receivers retain local objects and cannot move to another worker.
pub struct Receiver<T: Transfer> {
    payloads: platform::Receiver<T::Payload>,
    states: mpsc::UnboundedReceiver<T::State>,
    pending: Option<T::State>,
}

/// Creates an unbounded channel with one sender and one receiver.
///
/// Create the channel on its receiving thread and transfer the sender to its
/// producer. The sender can itself be a message on an existing channel. Thread
/// creation and channel creation are independent.
///
/// ```no_run
/// use relocade_workers::channel;
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let (mut sender, mut receiver) = channel::<String>()?;
/// sender.send("ready".to_owned())?;
/// assert_eq!(receiver.recv().await.transpose()?, Some("ready".to_owned()));
/// # Ok(())
/// # }
/// ```
///
/// Browser delivery needs an event-loop turn on the receiving thread; this
/// channel does not choose when guest execution yields to its host.
pub fn channel<T: Transfer>() -> Result<(Sender<T>, Receiver<T>), ChannelError> {
    let (sender, incoming) = endpoints()?;
    Ok((sender, incoming.bind()))
}

struct Incoming<T: Transfer> {
    payloads: platform::ReceiverEndpoint<T::Payload>,
    states: mpsc::UnboundedReceiver<T::State>,
}

impl<T: Transfer> Incoming<T> {
    fn bind(self) -> Receiver<T> {
        Receiver {
            payloads: self.payloads.bind(),
            states: self.states,
            pending: None,
        }
    }
}

fn endpoints<T: Transfer>() -> Result<(Sender<T>, Incoming<T>), ChannelError> {
    let (payloads, incoming_payloads) = platform::channel()?;
    let (states, incoming_states) = mpsc::unbounded();
    Ok((
        Sender { payloads, states },
        Incoming {
            payloads: incoming_payloads,
            states: incoming_states,
        },
    ))
}

pub(crate) fn on_worker<T: Transfer, F: Future + 'static>(
    thread: &Thread,
    code: impl FnOnce(Receiver<T>) -> F + Send + 'static,
) -> Result<(Sender<T>, SendTask<F::Output>), ChannelError>
where
    F::Output: Send,
{
    let (sender, Incoming { payloads, states }) = endpoints::<T>()?;
    let task = thread.run_send(payloads, move |payloads| {
        code(Incoming { payloads, states }.bind())
    });
    Ok((sender, task))
}

impl<T: Transfer> Transfer for Sender<T> {
    type State = mpsc::UnboundedSender<T::State>;
    type Payload = platform::Sender<T::Payload>;

    fn encode(self) -> (Self::State, Self::Payload) {
        (self.states, self.payloads)
    }

    fn decode(states: Self::State, payloads: Self::Payload) -> Self {
        Self { payloads, states }
    }
}

impl<T: Transfer> Sender<T> {
    /// Sends a value without waiting for receipt. Errors consume the value.
    /// Exclusive access preserves the ordering of payloads and Rust state.
    pub fn send(&mut self, value: T) -> Result<(), ChannelError> {
        if self.states.is_closed() {
            return Err(ChannelError::Closed);
        }
        let (state, payload) = value.encode();
        // Failed posts must not publish state for a payload that will never arrive.
        self.payloads.send(payload)?;
        self.states
            .unbounded_send(state)
            .map_err(|_| ChannelError::Closed)
    }
}

impl<T: Transfer> Receiver<T> {
    /// Receives the next value, or `None` after the sender and queued values end.
    /// Cancelling this wait preserves the next message, including Rust state
    /// already received while its browser payload is still in flight.
    /// Abrupt browser worker termination does not guarantee Rust cleanup or EOF.
    pub async fn recv(&mut self) -> Option<Result<T, ChannelError>> {
        std::future::poll_fn(|cx| self.poll_recv(cx)).await
    }

    fn poll_recv(&mut self, cx: &mut Context<'_>) -> Poll<Option<Result<T, ChannelError>>> {
        if self.pending.is_none() {
            let Some(state) = ready!(self.states.poll_next_unpin(cx)) else {
                return Poll::Ready(None);
            };
            self.pending = Some(state);
        }
        let payload = ready!(self.payloads.poll_recv(cx));
        let state = self.pending.take().expect("pending message state");
        Poll::Ready(Some(payload.map(|payload| T::decode(state, payload))))
    }
}
