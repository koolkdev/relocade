fn main() {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let application = relocade_app_tests::TestImage::default();
        let dropped = application.dropped.clone();
        let result =
            relocade_app::open(application, Default::default()).and_then(pollster::block_on);
        assert_eq!(dropped.load(std::sync::atomic::Ordering::Acquire), 3);
        match result {
            Ok(result) => println!("Application completed: {result:?}"),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
    }
}
