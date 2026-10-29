fn main() {
    #[cfg(not(target_arch = "wasm32"))]
    relocade_window_example::run().expect("run example window");
}
