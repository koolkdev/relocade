# Peripheral connections

Shared contracts connect device implementations to an application host without
requiring either to depend on the other. Implementations remain in their device
crates; the host owns its windows, workers and output lifecycle.

The `display` feature provides `display::DisplayRenderer`, the graphics-side
implementation of a display connection. `update(queue)` waits for source updates
and synchronizes inputs. `encode_image(encoder)` records GPU work for the current
image, or reuses an existing image. The host records drawing that image later in
the same batch and submits it before another update or encode. Recording returns
without waiting for GPU completion, and image storage is reusable. Discarding a
recorded batch does not roll back an implementation's image cache and can leave
stale pixels. Guest execution must progress independently of image requests.
The logical connection survives GPU replacement. The host calls `device_lost()`
to invalidate GPU state and let the backend notify its guest, then supplies a new
device through `attach_device(device, queue)`. Successful rebinding resumes
updates and image encoding, but does not imply guest recovery: the backend can keep
returning no image while processing a guest reset. VGA can restore a capture
automatically without imposing that policy on other implementations.

An existing `relocade_renderer::Image` can be used directly as static content.
The default lifecycle hooks reject device replacement; an image with only GPU
storage cannot reconstruct its contents.

This interface describes a consumer of display state. The emulated display device
owns registers, memory and timing. `relocade-renderer` provides the GPU image and
image-drawing tools used by the connection and host.
