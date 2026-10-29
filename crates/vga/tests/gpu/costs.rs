use super::capture;
use super::software;
use relocade_renderer::{Image, ImageBlitter};
use relocade_vga::VgaRenderer;
use std::{hint::black_box, sync::Arc, time::Instant};

// Explicit diagnostic, not a performance threshold. Completion measurements
// include driver/submission/wait costs, not just shader time, and do no readback.
#[test]
#[ignore = "reports adapter-dependent CPU and GPU completion costs"]
fn report_render_costs() {
    pollster::block_on(async {
        let instance = wgpu::Instance::default();
        let adapter = instance.request_adapter(&Default::default()).await.unwrap();
        eprintln!("adapter: {:?}", adapter.get_info());
        let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();
        let mut renderer = VgaRenderer::new(device.clone(), queue.clone()).unwrap();
        let uploaded = Image::new(&device, [640, 200], (5, 12)).unwrap();
        let output = Image::new(&device, [640, 480], (1, 1)).unwrap();
        let mut blitter = ImageBlitter::new(device.clone(), queue.clone(), Image::FORMAT).unwrap();
        let mut software = Vec::new();
        let mut upload_submit = Vec::new();
        let mut upload_complete = Vec::new();
        let mut decode_submit = Vec::new();
        let mut decode_complete = Vec::new();
        let mut blit_complete = Vec::new();
        for iteration in 0..80 {
            let mut input = capture();
            input.sequence = iteration;
            for (index, cell) in input.screen.cells.iter_mut().enumerate() {
                cell.character = index.wrapping_add(iteration as usize) as u8;
                cell.attribute = index as u8;
            }
            let input = Arc::new(input);
            let start = Instant::now();
            let pixels = black_box(software::rasterize(&input));
            let cpu_time = start.elapsed().as_secs_f64();

            let start = Instant::now();
            uploaded.upload(&queue, &pixels.pixels).unwrap();
            queue.submit([]);
            let upload_host_time = start.elapsed().as_secs_f64();
            device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
            let upload_time = start.elapsed().as_secs_f64();

            let start = Instant::now();
            let frame = renderer.render(&input);
            let decode_host_time = start.elapsed().as_secs_f64();
            device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
            let decode_time = start.elapsed().as_secs_f64();

            let start = Instant::now();
            blitter
                .blit(&frame.image, output.view(), output.size())
                .unwrap();
            device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
            let blit_time = start.elapsed().as_secs_f64();
            if iteration >= 16 {
                software.push(cpu_time);
                upload_submit.push(upload_host_time);
                upload_complete.push(upload_time);
                decode_submit.push(decode_host_time);
                decode_complete.push(decode_time);
                blit_complete.push(blit_time);
            }
        }
        for (label, mut samples) in [
            ("CPU rasterize", software),
            ("RGBA upload + submit (host)", upload_submit),
            ("RGBA upload through completion", upload_complete),
            ("text upload + decode submit (host)", decode_submit),
            ("text upload + decode through completion", decode_complete),
            ("640x480 blit through completion", blit_complete),
        ] {
            samples.sort_by(f64::total_cmp);
            eprintln!(
                "{label}: median {:.3} ms; p95 {:.3} ms (64 samples)",
                samples[samples.len() / 2] * 1000.0,
                samples[samples.len() * 95 / 100] * 1000.0,
            );
        }
    });
}
