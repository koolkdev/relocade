use super::{Endpoint, Receiver, Worker};
use crate::{
    requests::{Reply, Request},
    Error, Message, RequestEndpoint, RequestWorker, Result,
};
use std::{cell::RefCell, collections::VecDeque, marker::PhantomData, rc::Rc};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

type Replies<R> = Rc<RefCell<VecDeque<Reply<R>>>>;

pub(crate) struct Transport<C, R> {
    pub(crate) worker: Worker,
    replies: Replies<R>,
    input: PhantomData<fn(C)>,
}

pub(crate) struct Incoming<C, R> {
    endpoint: Endpoint,
    messages: PhantomData<fn(C) -> R>,
}

pub(crate) struct Response<R> {
    endpoint: Endpoint,
    output: PhantomData<fn(R)>,
}

impl<C: Message, R: Message> RequestWorker<C, R> {
    /// Uses a browser entry that calls `RequestEndpoint::serve` with matching
    /// request and response types. Message conversion belongs to those types.
    pub fn from_worker((worker, events): (Worker, Receiver), limit: usize) -> Self {
        let replies = Replies::default();
        spawn_local(receive_results(events, Rc::clone(&replies)));
        Self::new(
            Transport {
                worker,
                replies,
                input: PhantomData,
            },
            limit,
        )
    }
}

impl<C, R> From<Endpoint> for RequestEndpoint<C, R> {
    fn from(endpoint: Endpoint) -> Self {
        Self {
            incoming: Incoming {
                endpoint,
                messages: PhantomData,
            },
        }
    }
}

impl<C: Message, R> Transport<C, R> {
    pub(crate) fn send(&self, request: C, reply: Reply<R>) -> Result<()> {
        let message = request.encode();
        self.worker.send(&message.value, &message.transfer)?;
        self.replies.borrow_mut().push_back(reply);
        Ok(())
    }
}

impl<C: Message, R> Incoming<C, R> {
    pub(crate) fn ready(&self) {
        self.endpoint.ready();
    }

    pub(crate) async fn recv(&mut self) -> Result<Option<Request<C, R>>> {
        self.endpoint
            .recv()
            .await?
            .map(|value| {
                Ok(Request {
                    input: C::decode(value)?,
                    response: Response {
                        endpoint: self.endpoint.clone().unchecked_into(),
                        output: PhantomData,
                    },
                })
            })
            .transpose()
    }
}

impl<R: Message> Response<R> {
    pub(crate) fn send(self, result: Result<R>) -> Result<()> {
        let message = result.encode();
        self.endpoint.send(&message.value, &message.transfer)
    }
}

async fn receive_results<R: Message>(mut events: Receiver, replies: Replies<R>) {
    let failure = loop {
        match events.recv().await {
            Ok(Some(value)) => {
                // A dropped waiter keeps its slot until the corresponding reply.
                let reply = replies
                    .borrow_mut()
                    .pop_front()
                    .expect("reply has a request");
                reply.complete(Result::<R>::decode(value).and_then(|result| result));
            }
            Ok(None) => break Error::Closed,
            Err(error) => break error,
        }
    };
    for reply in replies.borrow_mut().drain(..) {
        reply.complete(Err(failure.clone()));
    }
}
