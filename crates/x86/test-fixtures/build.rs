use std::{env, fs, path::PathBuf};
use wasm86_x86 as x86;

mod targets;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=targets.rs");
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let mut modules = String::from("&[\n");
    for (index, (target, entry)) in targets::targets().into_iter().enumerate() {
        let compiler = x86::Compiler::new(target.profile).with_cpu_model(target.cpu_model);
        let module = match entry {
            targets::InterpreterEntry::Step => compiler.compile_interpreter_step(),
            targets::InterpreterEntry::Run => compiler.compile_interpreter(),
        }
        .expect("generate interpreter fixture");
        let path = out.join(format!("interpreter-{index}.wasm"));
        fs::write(&path, module.bytes).unwrap();
        modules.push_str(&format!(
            "(include_bytes!({:?}), {:?}),\n",
            path, module.entry
        ));
    }
    modules.push_str("]\n");
    fs::write(out.join("interpreters.rs"), modules).unwrap();
}
