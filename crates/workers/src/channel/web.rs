use crate::{Post, PostExt, ResourceError};
use futures_channel::mpsc;
use futures_util::StreamExt;
use js_sys::Array;
use std::{
    marker::PhantomData,
    task::{Context, Poll},
};
use wasm_bindgen::{closure::Closure, JsCast, JsValue};
use web_sys::{MessageChannel, MessageEvent, MessagePort};

#[path = "web/receivers.rs"]
mod receivers;
pub use receivers::Receiver;

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(transparent, bound = "")]
pub struct Sender<P> {
    #[serde(with = "serde_wasm_bindgen::preserve")]
    port: MessagePort,
    #[serde(skip)]
    marker: PhantomData<fn() -> P>,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(transparent, bound = "")]
pub struct ReceiverEndpoint<P> {
    #[serde(with = "serde_wasm_bindgen::preserve")]
    port: MessagePort,
    #[serde(skip)]
    marker: PhantomData<fn() -> P>,
}

struct Inbox<P> {
    port: MessagePort,
    incoming: mpsc::UnboundedReceiver<Result<P, ResourceError>>,
    _listener: Closure<dyn FnMut(MessageEvent)>,
    _errors: Closure<dyn FnMut(MessageEvent)>,
}

pub fn channel<P: Post + 'static>() -> Result<(Sender<P>, ReceiverEndpoint<P>), ResourceError> {
    let channel = MessageChannel::new().map_err(diagnostic)?;
    Ok((
        Sender {
            port: channel.port1(),
            marker: PhantomData,
        },
        ReceiverEndpoint {
            port: channel.port2(),
            marker: PhantomData,
        },
    ))
}

impl<P: Post> Sender<P> {
    pub fn send(&self, posted: P) -> Result<(), ResourceError> {
        self.port.post(posted).map_err(diagnostic)
    }
}

impl<P: Post + 'static> ReceiverEndpoint<P> {
    pub fn bind(self) -> Receiver<P> {
        let (messages, incoming) = mpsc::unbounded();
        let errors = messages.clone();
        let errors = Closure::<dyn FnMut(MessageEvent)>::new(move |_| {
            let _ = errors.unbounded_send(Err(ResourceError::Transport(
                "could not deserialize a worker resource".into(),
            )));
        });
        let listener = Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
            let _ = messages.unbounded_send(P::from_js(event.data()).map_err(diagnostic));
        });
        self.port
            .set_onmessage(Some(listener.as_ref().unchecked_ref()));
        self.port
            .set_onmessageerror(Some(errors.as_ref().unchecked_ref()));
        self.port.start();
        Receiver::register(Inbox {
            port: self.port,
            incoming,
            _listener: listener,
            _errors: errors,
        })
    }
}

impl<P> Inbox<P> {
    fn poll_recv(&mut self, cx: &mut Context<'_>) -> Poll<Result<P, ResourceError>> {
        self.incoming
            .poll_next_unpin(cx)
            .map(|posted| posted.unwrap_or(Err(ResourceError::Closed)))
    }
}

impl<P> Drop for Inbox<P> {
    fn drop(&mut self) {
        self.port.set_onmessage(None);
        self.port.set_onmessageerror(None);
        self.port.close();
    }
}

impl<P: 'static> Post for Sender<P> {
    fn transferables(&self) -> Array {
        Array::of1(self.port.as_ref())
    }
}

impl<P: 'static> Post for ReceiverEndpoint<P> {
    fn transferables(&self) -> Array {
        Array::of1(self.port.as_ref())
    }
}

fn diagnostic(error: JsValue) -> ResourceError {
    ResourceError::Transport(format!("{error:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[wasm_bindgen_test::wasm_bindgen_test]
    async fn message_errors_are_reported_to_the_receiver() {
        let (_sender, endpoint) = channel::<u32>().unwrap();
        let port = endpoint.port.clone();
        let mut receiver = endpoint.bind();
        port.dispatch_event(&MessageEvent::new("messageerror").unwrap())
            .unwrap();
        assert!(matches!(
            std::future::poll_fn(|cx| receiver.poll_recv(cx)).await,
            Err(ResourceError::Transport(_))
        ));
    }
}
