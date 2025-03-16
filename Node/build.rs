extern crate bindgen;

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=libsigner/libsigner.h");

    let bindings = bindgen::Builder::default()
        .header("libsigner/libsigner.h") // Define the header with function prototypes
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .generate()
        .expect("Unable to generate bindings");

    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");

    let lib_src = PathBuf::from("libsigner/libsigner.so");
    let profile = env::var("PROFILE").unwrap();
    let target_dir = if profile == "release" {
        "target/release/"
    } else {
        "target/debug/"
    };
    let lib_dst = PathBuf::from(format!("{}/libsigner.so", target_dir));
    fs::copy(&lib_src, &lib_dst).expect("Failed to copy libsigner.so");

    // Link to the Go C shared library
    println!("cargo:rustc-link-lib=dylib=signer");
    println!("cargo:rustc-link-search=native=libsigner");
}
