//! Win32 handles stay owned here; callers only wait or send a notification.
#![allow(unsafe_code)]

use std::{
    io,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    ptr,
    sync::Arc,
    task::{Wake, Waker},
};
use web_time::Instant;
use windows_sys::Win32::{
    Foundation::{FALSE, WAIT_OBJECT_0},
    System::Threading::{
        CancelWaitableTimer, CreateEventW, CreateWaitableTimerExW, SetEvent, SetWaitableTimer,
        WaitForMultipleObjects, CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, INFINITE,
        SYNCHRONIZATION_SYNCHRONIZE, TIMER_MODIFY_STATE,
    },
};

pub(super) struct Wait {
    timer: OwnedHandle,
    signal: Arc<Signal>,
}

impl Wait {
    pub fn new() -> Option<Self> {
        // Rust's timed parking uses WaitOnAddress, whose timeout can round short
        // PIT waits up to a system tick. It does not request a process timer-resolution
        // change. Older Windows versions fall back to ordinary parking.
        // SAFETY: Unnamed, non-inheritable timer; no borrowed pointers are retained.
        let timer = unsafe {
            CreateWaitableTimerExW(
                ptr::null(),
                ptr::null(),
                CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
                TIMER_MODIFY_STATE | SYNCHRONIZATION_SYNCHRONIZE,
            )
        };
        if timer.is_null() {
            return None;
        }
        // SAFETY: Each successful creation transfers one owned handle to us.
        let timer = unsafe { OwnedHandle::from_raw_handle(timer) };
        // SAFETY: Unnamed auto-reset event with no inherited security attributes.
        let event = unsafe { CreateEventW(ptr::null(), FALSE, FALSE, ptr::null()) };
        if event.is_null() {
            return None;
        }
        // SAFETY: Creation succeeded and this is the handle's sole owner.
        let event = unsafe { OwnedHandle::from_raw_handle(event) };
        Some(Self {
            timer,
            signal: Arc::new(Signal(event)),
        })
    }

    pub fn waker(&self) -> Waker {
        Waker::from(self.signal.clone())
    }

    pub fn until(&self, deadline: Option<Instant>) {
        let handles = [self.signal.0.as_raw_handle(), self.timer.as_raw_handle()];
        let count = if let Some(deadline) = deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return;
            }
            // Negative times are relative 100 ns intervals. Round up so a
            // nonzero wait never becomes zero, which denotes an absolute time.
            let intervals = remaining.as_nanos().div_ceil(100).min(i64::MAX as u128) as i64;
            let due = -intervals;
            // SAFETY: The owned timer is live, `due` is read during the call, and
            // no callback or pointer into Rust state is registered.
            let armed = unsafe {
                SetWaitableTimer(
                    self.timer.as_raw_handle(),
                    &due,
                    0,
                    None,
                    ptr::null(),
                    FALSE,
                )
            };
            assert_ne!(
                armed,
                0,
                "arming timer failed: {}",
                io::Error::last_os_error()
            );
            2
        } else {
            1
        };

        // The auto-reset event retains a notification sent before this wait.
        // Put it first so simultaneous timer and request signals service the request.
        // SAFETY: `count` selects one or both live, distinct handles. Neither can
        // close while this borrow is active, including when another thread wakes us.
        let result = unsafe { WaitForMultipleObjects(count, handles.as_ptr(), FALSE, INFINITE) };
        assert!(
            (WAIT_OBJECT_0..WAIT_OBJECT_0 + count).contains(&result),
            "timer wait failed: {}",
            io::Error::last_os_error()
        );
        if count == 2 && result == WAIT_OBJECT_0 {
            // SAFETY: The timer remains owned by this wait.
            let cancelled = unsafe { CancelWaitableTimer(self.timer.as_raw_handle()) };
            assert_ne!(
                cancelled,
                0,
                "cancelling timer failed: {}",
                io::Error::last_os_error()
            );
        }
        // Cancellation need not clear an already signaled timer. Idle waits use
        // only the event; SetWaitableTimer resets the timer on the next deadline.
    }
}

struct Signal(OwnedHandle);

impl Wake for Signal {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        // SAFETY: This Arc keeps the event open for the entire call.
        let signaled = unsafe { SetEvent(self.0.as_raw_handle()) };
        assert_ne!(
            signaled,
            0,
            "timer notification failed: {}",
            io::Error::last_os_error()
        );
    }
}
