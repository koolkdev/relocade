use futures_channel::mpsc::TryRecvError;
use relocade_app::{graphics::redraw_channel, winit::dpi::PhysicalSize, WindowState};

fn output(width: u32) -> WindowState {
    WindowState {
        size: PhysicalSize::new(width, 480),
        scale_factor: 1.0,
        occluded: false,
        close_requested: false,
    }
}

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn slow_graphics_receives_one_pending_draw_with_the_latest_size() {
    let (mut redraw, mut incoming) = redraw_channel(output(640));
    assert!(incoming.try_recv() == Err(TryRecvError::Empty));
    redraw.request().unwrap();
    assert_eq!(incoming.try_recv().unwrap(), PhysicalSize::new(640, 480));
    for width in 1..=1000 {
        redraw.output(output(width));
        redraw.request().unwrap();
    }
    assert!(
        incoming.try_recv() == Err(TryRecvError::Empty),
        "no queued work while a draw is in flight"
    );
    redraw.output(output(1280)); // A resize after the most recent redraw request.
    redraw.completed().unwrap();
    assert_eq!(incoming.try_recv().unwrap(), PhysicalSize::new(1280, 480));
    redraw.completed().unwrap();
    assert!(
        incoming.try_recv() == Err(TryRecvError::Empty),
        "completion alone is not redraw demand"
    );
    redraw.output(output(800));
    assert!(
        incoming.try_recv() == Err(TryRecvError::Empty),
        "layout alone is not redraw demand"
    );
    redraw.request().unwrap();
    assert_eq!(incoming.try_recv().unwrap(), PhysicalSize::new(800, 480));
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn hidden_zero_size_and_closing_output_do_not_dispatch() {
    let (mut redraw, mut incoming) = redraw_channel(output(0));
    redraw.request().unwrap();
    assert!(incoming.try_recv() == Err(TryRecvError::Empty));
    let mut state = output(640);
    state.occluded = true;
    redraw.output(state);
    redraw.request().unwrap();
    assert!(incoming.try_recv() == Err(TryRecvError::Empty));
    redraw.output(output(640));
    assert!(incoming.try_recv() == Err(TryRecvError::Empty));
    redraw.request().unwrap();
    assert_eq!(incoming.try_recv().unwrap(), PhysicalSize::new(640, 480));
    redraw.request().unwrap();
    state.close_requested = true;
    state.occluded = false;
    redraw.output(state);
    redraw.completed().unwrap();
    assert!(incoming.try_recv() == Err(TryRecvError::Empty));
    drop(redraw);
    assert_eq!(incoming.try_recv(), Err(TryRecvError::Closed));
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn shutdown_settles_the_in_flight_request_and_closes_the_channel() {
    let (mut redraw, mut incoming) = redraw_channel(output(640));
    redraw.request().unwrap();
    redraw.request().unwrap();
    drop(redraw);
    assert_eq!(incoming.try_recv().unwrap(), PhysicalSize::new(640, 480));
    assert_eq!(incoming.try_recv(), Err(TryRecvError::Closed));

    let (mut redraw, incoming) = redraw_channel(output(640));
    drop(incoming);
    assert!(redraw.request().unwrap_err().is_disconnected());
}
