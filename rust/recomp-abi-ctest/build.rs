fn main() {
    println!("cargo:rerun-if-changed=c/reference.c");
    println!("cargo:rerun-if-changed=../../include/recomp.h");
    cc::Build::new()
        .file("c/reference.c")
        .include("../../include")
        .compile("recomp_reference");
    println!("cargo:rustc-link-lib=m");
}
