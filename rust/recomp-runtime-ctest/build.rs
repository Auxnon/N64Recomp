fn main() {
    for file in [
        "c/runtime_reference.cpp",
        "c/save_reference.cpp",
        "c/save_backend.cpp",
        "c/save_prelude.hpp",
        "c/pi_reference.cpp",
        "c/saving_reference.cpp",
    ] {
        println!("cargo:rerun-if-changed={file}");
    }
    println!("cargo:rerun-if-changed=../../include/recomp.h");
    cc::Build::new()
        .cpp(true)
        .file("c/runtime_reference.cpp")
        .file("c/save_reference.cpp")
        .file("c/save_backend.cpp")
        .file("c/pi_reference.cpp")
        .file("c/saving_reference.cpp")
        .flag_if_supported("-std=c++20")
        // As games build the runtime: release, so `assert` compiles out.
        .define("NDEBUG", None)
        .include("../../include")
        .compile("runtime_reference");
}
