# Links the Rust runtime crate (rust/recomp-runtime) into a game built on N64ModernRuntime.
#
#   include(/path/to/n64recomp/rust/integration/RecompRustRuntime.cmake)
#   recomp_link_rust_runtime(librecomp)
#
# The runtime sources need the guards from runtime.patch (next to this file): with
# RECOMP_RUST_RUNTIME defined, librecomp drops the C++ definitions recomp-runtime replaces
# and registers RDRAM with recomp_abi_install_guest_memory once it is mapped.

set(RECOMP_RUST_DIR "${CMAKE_CURRENT_LIST_DIR}/.." CACHE PATH "Path to the n64recomp Rust workspace")
set(RECOMP_RUST_TARGET_DIR "${CMAKE_BINARY_DIR}/rust-target")
set(RECOMP_RUST_LIB "${RECOMP_RUST_TARGET_DIR}/release/librecomp_runtime.a")

find_program(CARGO cargo REQUIRED)

# Always runs; cargo itself decides whether anything needs rebuilding.
add_custom_target(recomp_runtime_cargo
    COMMAND "${CARGO}" build --release -p recomp-runtime
            --manifest-path "${RECOMP_RUST_DIR}/Cargo.toml"
            --target-dir "${RECOMP_RUST_TARGET_DIR}"
    BYPRODUCTS "${RECOMP_RUST_LIB}"
    USES_TERMINAL
)

add_library(recomp_runtime_rs STATIC IMPORTED GLOBAL)
set_target_properties(recomp_runtime_rs PROPERTIES
    IMPORTED_LOCATION "${RECOMP_RUST_LIB}"
    # System libraries Rust's std needs (`cargo rustc -- --print native-static-libs`).
    INTERFACE_LINK_LIBRARIES "pthread;dl;m;gcc_s;rt;util"
)

function(recomp_link_rust_runtime librecomp_target)
    add_dependencies(${librecomp_target} recomp_runtime_cargo)
    target_compile_definitions(${librecomp_target} PRIVATE RECOMP_RUST_RUNTIME)
    target_link_libraries(${librecomp_target} PUBLIC recomp_runtime_rs)
endfunction()
