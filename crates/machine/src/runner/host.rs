use super::Runner;
use crate::worker;

impl Runner {
    /// Binds a resource channel to this worker once. Receipt uses Rust readiness
    /// to defer host turns until a posted object actually needs browser delivery.
    /// Host-backed sessions allow one millisecond before yielding; sessions with
    /// no independent host timer yield immediately. Guest time is never used.
    pub fn bind_resource<R: worker::WorkerResource>(
        &self,
        endpoint: worker::ReceiverEndpoint<R>,
    ) -> worker::Receiver<R> {
        endpoint.bind_on(&self.executor)
    }

    /// Receives a resource posted after this worker started. The handoff owns
    /// posting readiness, host delivery and completion of its sending task.
    pub async fn receive_resource<R: worker::WorkerResource>(
        &self,
        endpoint: worker::oneshot::ReceiverEndpoint<R>,
    ) -> worker::Result<R, worker::ResourceError> {
        endpoint.bind_on(&self.executor)?.receive().await
    }
}
