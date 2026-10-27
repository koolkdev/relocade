use super::*;
use futures_channel::oneshot;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};

wasm_bindgen_test_configure!(run_in_browser);

struct ControlFixture<T> {
    control: Control<T>,
    finish: oneshot::Sender<Result<Exit<T>, String>>,
    stopped: Arc<AtomicBool>,
}

fn control<T>() -> ControlFixture<T> {
    let stopped = Arc::new(AtomicBool::new(false));
    let requested = stopped.clone();
    let (finish, completion) = oneshot::channel();
    ControlFixture {
        control: Control {
            stop: StopHandle(Arc::new(move || requested.store(true, Ordering::SeqCst))),
            completion,
        },
        finish,
        stopped,
    }
}

#[wasm_bindgen_test]
async fn stop_and_typed_completion_use_the_same_control() {
    struct Report(String);
    let ControlFixture {
        control,
        finish,
        stopped,
    } = control();
    let browser = launch(Ok(control), |exit| match exit {
        Exit::Completed(Report(value)) => value.into(),
        Exit::Cancelled => JsValue::NULL,
    })
    .unwrap();
    let finished = browser.finished();
    assert!(js_sys::Object::is(&finished, &browser.finished()));
    browser.stop();
    assert!(stopped.load(Ordering::SeqCst));
    // A shutdown request does not replace a result retained by the host.
    assert!(finish
        .send(Ok(Exit::Completed(Report("retained".into()))))
        .is_ok());
    assert_eq!(JsFuture::from(finished).await.unwrap(), "retained");
}

#[wasm_bindgen_test]
async fn cancellation_uses_the_application_mapping() {
    let ControlFixture {
        control, finish, ..
    } = control::<()>();
    let browser = launch(Ok(control), |exit| match exit {
        Exit::Completed(()) => JsValue::TRUE,
        Exit::Cancelled => "cancelled".into(),
    })
    .unwrap();
    finish.send(Ok(Exit::Cancelled)).unwrap();
    assert_eq!(
        JsFuture::from(browser.finished()).await.unwrap(),
        "cancelled"
    );
}

#[wasm_bindgen_test]
async fn launch_and_worker_failures_are_javascript_errors() {
    let error = launch::<()>(Err("launch failed".into()), |_| unreachable!())
        .err()
        .unwrap()
        .dyn_into::<js_sys::Error>()
        .unwrap();
    assert_eq!(String::from(error.message()), "launch failed");
    let ControlFixture {
        control, finish, ..
    } = control::<()>();
    let browser = launch(Ok(control), |_| unreachable!()).unwrap();
    finish.send(Err("worker failed".into())).unwrap();
    let error = JsFuture::from(browser.finished())
        .await
        .unwrap_err()
        .dyn_into::<js_sys::Error>()
        .unwrap();
    assert_eq!(String::from(error.message()), "worker failed");
}
