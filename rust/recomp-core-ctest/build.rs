fn main() {
    println!("cargo:rerun-if-changed=cxx/reference.cpp");
    cc::Build::new()
        .cpp(true)
        .std("c++20")
        .file("cxx/reference.cpp")
        .include("../../lib/fmt/include")
        .define("FMT_HEADER_ONLY", None)
        .compile("recomp_core_reference");
}
