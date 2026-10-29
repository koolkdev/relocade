pub(crate) mod gpu;
mod guest;
pub(crate) mod target;

use crate::software::{self, SoftwareFrame};
use futures_channel::{mpsc, oneshot};
use futures_util::{future::join, FutureExt, StreamExt};
use relocade_machine::{timer_service, worker::Thread};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};
use target::Target;

#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub async fn graphics_workers() -> Result<(), String> {
    let (clock, timers) = timer_service();

    let app = async move {
        let guest::Guest {
            execution,
            source,
            progress,
        } = guest::spawn(clock).await;
        // A single Sender with buffer size zero has one reserved request slot.
        // This fixture app drops excess requests; VGA owns no request protocol.
        let (mut requests, mut incoming) = mpsc::channel::<[u32; 2]>(0);
        let (selected, selected_capture) = oneshot::channel();
        let (convert, conversion_allowed) = oneshot::channel();
        let (resume, resumed) = oneshot::channel();
        let (report, mut reports) = mpsc::unbounded();
        let conversions = Arc::new(AtomicUsize::new(0));
        let conversion_count = conversions.clone();
        let dropped = Arc::new(AtomicBool::new(false));
        let target_drop = dropped.clone();
        let worker_source = source.clone();
        let graphics = Thread::new();
        let task = graphics.run_send(target::context()?, move |context| async move {
            let mut target = Target::new(context, target_drop).await?;
            let mut cached: Option<Arc<SoftwareFrame>> = None;
            let mut before_conversion = Some((selected, conversion_allowed));
            let mut after_present = Some(resumed);
            while let Some(size) = incoming.next().await {
                let Some(capture) = worker_source.next_capture(None).await else {
                    continue;
                };
                if let Some((selected, allowed)) = before_conversion.take() {
                    selected.send(capture.sequence).unwrap();
                    allowed.await.unwrap(); // Hold the first conversion, not the CPU.
                }
                if cached
                    .as_ref()
                    .is_none_or(|image| image.sequence != capture.sequence)
                {
                    conversion_count.fetch_add(1, Ordering::Release);
                    cached = Some(Arc::new(software::rasterize(&capture)));
                }
                let image = cached.as_ref().unwrap().clone();
                let presented = target.present(&image, size).await?;
                report.unbounded_send((presented, image)).unwrap();
                if let Some(resumed) = after_present.take() {
                    resumed.await.unwrap();
                }
            }
            Ok::<(), String>(())
        });

        request(&mut requests, [64, 48]);
        let selected = selected_capture.await.unwrap();
        let mut sequence = source.next_capture(Some(selected)).await.unwrap().sequence;
        let before = progress.load(Ordering::Acquire);
        for _ in 0..10 {
            for _ in 0..100 {
                request(&mut requests, [96, 72]);
            }
            sequence = source.next_capture(Some(sequence)).await.unwrap().sequence;
        }
        assert!(progress.load(Ordering::Acquire) > before);
        assert_eq!(conversions.load(Ordering::Acquire), 0);
        convert.send(()).unwrap();
        let (first, first_image) = reports.next().await.unwrap();
        assert_eq!(first.sequence, selected);
        assert_eq!(first.size, [64, 48]);
        assert_eq!(first.pixel, [255, 255, 85, 255]);

        // Holding presentation also leaves only one pending request. The next
        // conversion selects a recent capture rather than replaying a backlog.
        let before = progress.load(Ordering::Acquire);
        for _ in 0..10 {
            for _ in 0..100 {
                request(&mut requests, [96, 72]);
            }
            sequence = source.next_capture(Some(sequence)).await.unwrap().sequence;
        }
        assert!(progress.load(Ordering::Acquire) > before);
        assert_eq!(conversions.load(Ordering::Acquire), 1);
        resume.send(()).unwrap();
        let (second, retained) = reports.next().await.unwrap();
        assert!(second.sequence >= sequence);
        assert_eq!(second.size, [96, 72]);
        assert_eq!(second.count, 2);
        assert_eq!(second.pixel, first.pixel);
        assert!(!Arc::ptr_eq(&first_image, &retained));

        // No requests: captures and the CPU progress without any conversion.
        let before = progress.load(Ordering::Acquire);
        sequence = second.sequence;
        for _ in 0..10 {
            sequence = source.next_capture(Some(sequence)).await.unwrap().sequence;
        }
        assert!(progress.load(Ordering::Acquire) > before);
        assert_eq!(conversions.load(Ordering::Acquire), 2);
        assert!(reports.next().now_or_never().is_none());
        request(&mut requests, [48, 36]);
        let (third, _) = reports.next().await.unwrap();
        assert!(third.sequence >= sequence);
        assert_eq!(third.size, [48, 36]);
        assert_eq!(third.count, 3);
        assert_eq!(third.pixel, first.pixel);

        // Source closure does not close the host's surface or its worker. The
        // application may render the last capture and present it repeatedly.
        assert_eq!(execution.stop().await.map_err(|e| e.to_string())?, None);
        let last = source.current_capture().unwrap();
        assert!(source.next_capture(Some(last.sequence)).await.is_none());
        assert!(!dropped.load(Ordering::Acquire));
        request(&mut requests, [64, 48]);
        let (closed, last_image) = reports.next().await.unwrap();
        assert_eq!(closed.sequence, last.sequence);
        let count = conversions.load(Ordering::Acquire);
        request(&mut requests, [64, 48]);
        let (repeated, repeated_image) = reports.next().await.unwrap();
        assert_eq!(repeated.count, closed.count + 1);
        assert!(Arc::ptr_eq(&last_image, &repeated_image));
        assert_eq!(conversions.load(Ordering::Acquire), count);
        assert!(!dropped.load(Ordering::Acquire));
        drop(requests); // Only the host decides when presentation ends.
        task.await.map_err(|e| e.to_string())??;
        assert!(dropped.load(Ordering::Acquire));
        Ok(())
    };
    let ((), result) = join(timers.run(), app).await;
    result
}

// Coalescing is a fixture policy. There is one sender, hence one pending slot;
// a full channel already represents the requested redraw opportunity.
pub(crate) fn request(requests: &mut mpsc::Sender<[u32; 2]>, size: [u32; 2]) {
    if let Err(error) = requests.try_send(size) {
        assert!(error.is_full(), "graphics worker must still be listening");
    }
}
