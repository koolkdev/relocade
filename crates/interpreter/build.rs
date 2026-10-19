use relocade_x86::{Compiler, ExecutionProfile, SegmentProfile};
use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo output directory"));
    for (name, profile) in [
        ("flat32", SegmentProfile::Flat32.into()),
        ("segmented32", SegmentProfile::Segmented32.into()),
        ("segmented16", SegmentProfile::Segmented16.into()),
        ("real16", ExecutionProfile::Real16),
    ] {
        let module = Compiler::new(profile)
            .compile_interpreter()
            .expect("generate interpreter module");
        fs::write(out.join(format!("{name}.wasm")), module.bytes)
            .expect("write interpreter module");
    }
}
