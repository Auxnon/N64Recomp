fn main() {
    println!("cargo:rerun-if-changed=c/runtime_reference.cpp");
    println!("cargo:rerun-if-changed=../../include/recomp.h");
    cc::Build::new()
        .cpp(true)
        .file("c/runtime_reference.cpp")
        .include("../../include")
        .compile("runtime_reference");
}
