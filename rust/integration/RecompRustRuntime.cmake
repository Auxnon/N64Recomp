# Links the Rust runtime crate (rust/recomp-runtime) into a game built on N64ModernRuntime.
#
#   include(/path/to/n64recomp/rust/integration/RecompRustRuntime.cmake)
#   recomp_link_rust_runtime(librecomp)
#   recomp_add_fast3d_renderer(MyGame)   # only with -DRECOMP_RUST_FAST3D=ON
#
# The runtime sources need the guards from runtime.patch (next to this file): with
# RECOMP_RUST_RUNTIME defined, librecomp drops the C++ definitions recomp-runtime replaces
# and registers RDRAM with recomp_abi_install_guest_memory once it is mapped.

set(RECOMP_RUST_DIR "${CMAKE_CURRENT_LIST_DIR}/.." CACHE PATH "Path to the n64recomp Rust workspace")
set(RECOMP_RUST_TARGET_DIR "${CMAKE_BINARY_DIR}/rust-target")
set(RECOMP_RUST_LIB "${RECOMP_RUST_TARGET_DIR}/release/librecomp_runtime.a")

option(RECOMP_RUST_FAST3D "Include the fast3d-rs renderer (rust/recomp-render) in the Rust runtime" OFF)
# A game that emulates the TLB defines osUnmapTLBAll itself; set this OFF for it, or the
# runtime's empty one collides with it at link time.
option(RECOMP_RUST_TLB_STUB "Include the runtime's empty osUnmapTLBAll" ON)
set(RECOMP_RUST_FEATURE_LIST "")
if (RECOMP_RUST_TLB_STUB)
    list(APPEND RECOMP_RUST_FEATURE_LIST tlb-stub)
endif()
if (RECOMP_RUST_FAST3D)
    list(APPEND RECOMP_RUST_FEATURE_LIST fast3d)
endif()
list(JOIN RECOMP_RUST_FEATURE_LIST "," RECOMP_RUST_FEATURE_CSV)
set(RECOMP_RUST_FEATURES --no-default-features --features "${RECOMP_RUST_FEATURE_CSV}")

find_program(CARGO cargo REQUIRED)

# Always runs; cargo itself decides whether anything needs rebuilding.
add_custom_target(recomp_runtime_cargo
    COMMAND "${CARGO}" build --release -p recomp-runtime ${RECOMP_RUST_FEATURES}
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

# Adds the C++ RendererContext that forwards to fast3d (fast3d_render_context.cpp) to the game
# target and defines RECOMP_FAST3D there. The game's create_render_context picks it at runtime.
function(recomp_add_fast3d_renderer game_target)
    if (NOT RECOMP_RUST_FAST3D)
        message(FATAL_ERROR "recomp_add_fast3d_renderer needs -DRECOMP_RUST_FAST3D=ON")
    endif()
    target_sources(${game_target} PRIVATE "${RECOMP_RUST_DIR}/integration/fast3d_render_context.cpp")
    target_include_directories(${game_target} PRIVATE
        "${RECOMP_RUST_DIR}/integration"
        "${RECOMP_RUST_DIR}/recomp-render/include")
    target_compile_definitions(${game_target} PRIVATE RECOMP_FAST3D)
endfunction()
