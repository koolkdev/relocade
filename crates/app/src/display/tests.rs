use super::{await_host, RendererState};
use crate::{Graphics, Renderer, Texture};
use futures_channel::oneshot;
use futures_util::{future, FutureExt};
use std::{
    cell::{Cell, RefCell},
    future::Future,
    marker::PhantomPinned,
    pin::Pin,
    task::{ready, Context, Poll},
};

// Interior mutability permits safe polling of a !Unpin, worker-local fixture.
struct RequestRenderer {
    request: RefCell<oneshot::Receiver<()>>,
    reply: RefCell<Option<oneshot::Sender<()>>>,
    retired: Option<oneshot::Sender<()>>,
    initial: Texture,
    rendered: Texture,
    complete: Cell<bool>,
    _pin: PhantomPinned,
}

impl Renderer for RequestRenderer {
    type Source = (
        oneshot::Receiver<()>,
        oneshot::Sender<()>,
        oneshot::Sender<()>,
    );

    fn new((request, reply, retired): Self::Source, graphics: Graphics) -> Result<Self, String> {
        Ok(Self {
            request: RefCell::new(request),
            reply: RefCell::new(Some(reply)),
            retired: Some(retired),
            initial: Texture::new(&graphics.device, [1, 1]).unwrap(),
            rendered: Texture::new(&graphics.device, [2, 3]).unwrap(),
            complete: Cell::new(false),
            _pin: PhantomPinned,
        })
    }

    fn texture(&self) -> Option<&Texture> {
        Some(if self.complete.get() {
            &self.rendered
        } else {
            &self.initial
        })
    }
}

impl Future for RequestRenderer {
    type Output = Result<(), String>;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        assert!(!self.complete.get(), "completed renderer was polled again");
        ready!(self.request.borrow_mut().poll_unpin(context)).unwrap();
        self.complete.set(true);
        self.reply.borrow_mut().take().unwrap().send(()).unwrap();
        Poll::Ready(Ok(()))
    }
}

impl Drop for RequestRenderer {
    fn drop(&mut self) {
        let _ = self.retired.take().unwrap().send(());
    }
}

struct FailingRenderer;

impl Renderer for FailingRenderer {
    type Source = ();

    fn new(_: (), _: Graphics) -> Result<Self, String> {
        Ok(Self)
    }

    fn texture(&self) -> Option<&Texture> {
        None
    }
}

impl Future for FailingRenderer {
    type Output = Result<(), String>;

    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
        Poll::Ready(Err("rendering failed".into()))
    }
}

#[test]
fn renderer_progress_and_completion_are_independent_of_host_work() {
    pollster::block_on(async {
        let instance = wgpu::Instance::default();
        let adapter = instance.request_adapter(&Default::default()).await.unwrap();
        let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();
        let graphics = Graphics { device, queue };
        let (request, incoming) = oneshot::channel();
        let (reply, completion) = oneshot::channel();
        let (retired, released) = oneshot::channel();
        let mut renderer = RendererState::Active(Box::pin(
            RequestRenderer::new((incoming, reply, retired), graphics.clone()).unwrap(),
        ));
        assert_eq!(renderer.texture().unwrap().size(), [1, 1]);
        // The renderer first waits for this host operation to start. The host
        // then waits for its reply and resource release. Surface recreation
        // uses this same production await path.
        let host = async {
            request.send(()).unwrap();
            completion.await.unwrap();
            released.await.unwrap();
            17
        };
        assert_eq!(await_host(&mut renderer, host).await.unwrap(), 17);
        assert_eq!(renderer.texture().unwrap().size(), [2, 3]);
        // Later host work can use the final texture without repolling the renderer.
        await_host(&mut renderer, future::ready(())).await.unwrap();
        assert_eq!(renderer.texture().unwrap().size(), [2, 3]);

        let mut renderer =
            RendererState::Active(Box::pin(FailingRenderer::new((), graphics).unwrap()));
        assert_eq!(
            await_host(&mut renderer, future::pending::<()>()).await,
            Err("rendering failed".into())
        );
    });
}
