fn main() {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let application = relocade_app_tests::TestImage::default();
        let observations = application.observations.clone();
        let result =
            relocade_app::open(application, Default::default()).and_then(pollster::block_on);
        match result {
            Ok(result) => {
                assert_eq!(
                    observations
                        .dropped
                        .load(std::sync::atomic::Ordering::Acquire),
                    3
                );
                println!("Application completed: {result:?}");
            }
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
    }
}
