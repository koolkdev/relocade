//! Typed messages with Rust state and posted objects delivered in the same order.

use crate::{host_turn::HostTurns, Executor, SendTask, Transfer, WakeTarget};
use futures_channel::mpsc;
use futures_util::StreamExt;
use std::{fmt, future::Future, rc::Rc};

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

/// Receives messages on the worker that bound this endpoint.
/// Browser receivers retain local objects and cannot move to another worker.
pub struct Receiver<T: Transfer> {
    payloads: platform::Receiver<T::Payload>,
    states: mpsc::UnboundedReceiver<T::State>,
    pending: Option<T::State>,
    host: Option<Rc<HostTurns>>,
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
    let (sender, incoming) = channel_endpoints()?;
    Ok((sender, incoming.bind()))
}

/// A receiving endpoint that can move between workers before it is bound.
/// Transfer it to the receiving worker, then call [`Self::bind`] there.
pub struct ReceiverEndpoint<T: Transfer> {
    payloads: platform::ReceiverEndpoint<T::Payload>,
    states: mpsc::UnboundedReceiver<T::State>,
}

impl<T: Transfer> ReceiverEndpoint<T> {
    /// Starts receiving on this worker. The resulting receiver stays local.
    pub fn bind(self) -> Receiver<T> {
        Receiver {
            payloads: self.payloads.bind(),
            states: self.states,
            pending: None,
            host: None,
        }
    }
}

impl<T: Transfer> Transfer for ReceiverEndpoint<T> {
    type State = mpsc::UnboundedReceiver<T::State>;
    type Payload = platform::ReceiverEndpoint<T::Payload>;

    fn encode(self) -> (Self::State, Self::Payload) {
        (self.states, self.payloads)
    }

    fn decode(states: Self::State, payloads: Self::Payload) -> Self {
        Self { payloads, states }
    }
}

/// Creates a channel before choosing its receiving worker.
/// Both endpoints can be transferred; bind the receiver on its final owner.
/// Use [`channel`] when the receiver belongs on the current worker.
pub fn channel_endpoints<T: Transfer>() -> Result<(Sender<T>, ReceiverEndpoint<T>), ChannelError> {
    let (payloads, incoming_payloads) = platform::channel()?;
    let (states, incoming_states) = mpsc::unbounded();
    Ok((
        Sender { payloads, states },
        ReceiverEndpoint {
            payloads: incoming_payloads,
            states: incoming_states,
        },
    ))
}

pub(crate) fn on_thread<T: Transfer, F: Future + 'static>(
    thread: &web_thread::Thread,
    code: impl FnOnce(Receiver<T>) -> F + Send + 'static,
) -> Result<(Sender<T>, SendTask<F::Output>), ChannelError>
where
    F::Output: Send,
{
    let (sender, ReceiverEndpoint { payloads, states }) = channel_endpoints::<T>()?;
    let task = thread.run_send(payloads, move |payloads| {
        code(ReceiverEndpoint { payloads, states }.bind())
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
    /// Binds posted-object receipt to this executor's host-turn policy.
    /// Poll receives inside its root future. Empty channels and already delivered
    /// payloads request no host turn; cancelling receipt preserves the message.
    pub fn with_executor<R: WakeTarget>(mut self, executor: &Executor<R>) -> Self {
        self.host = Some(executor.host_turns.clone());
        self
    }

    /// Receives the next value, or `None` after the sender and queued values end.
    /// Cancelling this wait preserves the next message, including Rust state
    /// already received while its browser payload is still in flight.
    /// Abrupt browser worker termination does not guarantee Rust cleanup or EOF.
    pub async fn recv(&mut self) -> Option<Result<T, ChannelError>> {
        if self.pending.is_none() {
            self.pending = Some(self.states.next().await?);
        }
        let receive = std::future::poll_fn(|cx| self.payloads.poll_recv(cx));
        let payload = match &self.host {
            Some(host) => host.receive(receive).await,
            None => receive.await,
        };
        let state = self.pending.take().expect("pending message state");
        Some(payload.map(|payload| T::decode(state, payload)))
    }
}

#[cfg(test)]
mod tests;
