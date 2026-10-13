fn main() {
    for file in [
        "c/runtime_reference.cpp",
        "c/save_reference.cpp",
        "c/save_backend.cpp",
        "c/save_prelude.hpp",
    ] {
        println!("cargo:rerun-if-changed={file}");
    }
    println!("cargo:rerun-if-changed=../../include/recomp.h");
    cc::Build::new()
        .cpp(true)
        .file("c/runtime_reference.cpp")
        .file("c/save_reference.cpp")
        .file("c/save_backend.cpp")
        .flag_if_supported("-std=c++17")
        // As games build the runtime: release, so `assert` compiles out.
        .define("NDEBUG", None)
        .include("../../include")
        .compile("runtime_reference");
}
