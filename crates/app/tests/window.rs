// Winit must run on main, so this check uses a standalone test entrypoint.
// Ordinary workspace tests skip it; CI opts in under Xvfb with --ignored.
fn main() {
    #[cfg(target_os = "linux")]
    {
        let arguments: Vec<_> = std::env::args().collect();
        if arguments.iter().any(|argument| argument == "--graphics") {
            linux::check(true);
        } else if arguments.iter().any(|argument| argument == "--immediate") {
            linux::check(false);
        } else if arguments.iter().any(|argument| argument == "--ignored") {
            // Native winit permits one event loop per process. Exercise both
            // completion paths in fresh processes under the same display server.
            for case in ["--graphics", "--immediate"] {
                assert!(std::process::Command::new(std::env::current_exe().unwrap())
                    .arg(case)
                    .status()
                    .unwrap()
                    .success());
            }
        }
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use futures_channel::oneshot;
    use relocade_app::{
        Application, Context, Exit, Graphics, OnCompletion, Renderer, Texture, WindowOptions,
    };
    use std::{
        future::Future,
        pin::Pin,
        sync::mpsc,
        task::{Context as PollContext, Poll},
        thread,
        time::Duration,
    };

    struct Fixture {
        wait_for_rendering: bool,
    }

    #[derive(Debug, Eq, PartialEq)]
    struct Report {
        size: [u32; 2],
    }

    struct LocalRenderer {
        graphics: Graphics,
        texture: Texture,
        rendered: Option<oneshot::Sender<[u32; 2]>>,
    }

    impl Renderer for LocalRenderer {
        type Source = oneshot::Sender<[u32; 2]>;

        fn new(rendered: Self::Source, graphics: Graphics) -> Result<Self, String> {
            // Texture dimensions do not depend on the host's 64-by-64 window.
            let texture =
                Texture::new(&graphics.device, [2, 3]).map_err(|error| error.to_string())?;
            Ok(Self {
                graphics,
                texture,
                rendered: Some(rendered),
            })
        }

        fn texture(&self) -> Option<&Texture> {
            Some(&self.texture)
        }
    }

    impl Future for LocalRenderer {
        type Output = Result<(), String>;

        fn poll(self: Pin<&mut Self>, _: &mut PollContext<'_>) -> Poll<Self::Output> {
            let this = self.get_mut();
            let mut encoder = this
                .graphics
                .device
                .create_command_encoder(&Default::default());
            {
                let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: this.texture.view(),
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: 0.0,
                                g: 0.0,
                                b: 1.0,
                                a: 1.0,
                            }),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                });
            }
            this.graphics.queue.submit([encoder.finish()]);
            let _ = this.rendered.take().unwrap().send(this.texture.size());
            Poll::Ready(Ok(()))
        }
    }

    impl Application for Fixture {
        type Update = oneshot::Sender<[u32; 2]>;
        type Output = Report;

        async fn run(self, context: Context<Self::Update>) -> Result<Exit<Report>, String> {
            let (rendered, rendering_done) = oneshot::channel();
            context.send(rendered);
            let size = if self.wait_for_rendering {
                rendering_done.await.map_err(|error| error.to_string())?
            } else {
                [0, 0]
            };
            Ok(Exit::Completed(Report { size }))
        }
    }

    pub(super) fn check(wait_for_rendering: bool) {
        let (finished, completion) = mpsc::channel();
        let watchdog = thread::spawn(move || {
            if matches!(
                completion.recv_timeout(Duration::from_secs(60)),
                Err(mpsc::RecvTimeoutError::Timeout)
            ) {
                eprintln!("application did not complete automatic window shutdown");
                std::process::exit(1);
            }
        });
        let control = relocade_app::run::<LocalRenderer, _>(
            Fixture { wait_for_rendering },
            WindowOptions {
                title: "Rendering completion".into(),
                size: [64, 64],
                on_completion: OnCompletion::Close,
            },
        )
        .unwrap();
        assert_eq!(
            pollster::block_on(control).unwrap(),
            Exit::Completed(Report {
                size: if wait_for_rendering { [2, 3] } else { [0, 0] },
            })
        );
        finished.send(()).unwrap();
        watchdog.join().unwrap();
    }
}
