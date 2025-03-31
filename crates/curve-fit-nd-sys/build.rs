use std::env;
use std::path::PathBuf;

fn main() {
    // Build the C library
    let mut build = cc::Build::new();
    build.include("curve-fit-nd/c");
    build.include("curve-fit-nd/c/intern");
    let source_files = [
        "curve-fit-nd/c/intern/curve_fit_corners_detect.c",
        "curve-fit-nd/c/intern/curve_fit_cubic.c",
        "curve-fit-nd/c/intern/curve_fit_cubic_refit.c",
        "curve-fit-nd/c/intern/generic_heap.c",
    ];
    for s in &source_files {
        build.file(s);
    }
    build.compile("curve-fit-nd");

    // Generate bindings
    let bindings = bindgen::Builder::default()
        .header("curve-fit-nd/c/curve_fit_nd.h")
        // Tell cargo to invalidate the built crate whenever any of the
        // included header files changed.
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .generate()
        .expect("Unable to generate bindings");

    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}
