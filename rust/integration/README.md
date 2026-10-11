# Linking the Rust runtime into Zelda64Recomp

These are the steps used to build Zelda64Recomp (`b65c482`, Majora's Mask US) against
`rust/recomp-runtime`. The checkout is expected next to this repo, at `../Zelda64Recomp`.

1. Clone and generate code as in Zelda64Recomp's `BUILDING.md`, using `N64Recomp` and
   `RSPRecomp` built from this repo.
2. Apply the patches:

   ```sh
   cd ../Zelda64Recomp
   git apply ../n64recomp/rust/integration/zelda64recomp.patch
   git -C lib/N64ModernRuntime apply ../../../n64recomp/rust/integration/runtime.patch
   ```

   - `zelda64recomp.patch` (Zelda64Recomp itself):
     - Adds the `RECOMP_RUST_RUNTIME` CMake option.
     - Downgrades clang 22's `-Wincompatible-pointer-types` error in `patches/Makefile`.
     - Drops `-static-libstdc++`, because Fedora doesn't install `libstdc++.a` by default.
   - `runtime.patch` (N64ModernRuntime): behind `RECOMP_RUST_RUNTIME`, removes the C++
     functions the Rust crate replaces and registers RDRAM with
     `recomp_abi_install_guest_memory`.
3. With clang 22, also move the runtime's pinned fmt to 12.1.0:
   `git -C lib/N64ModernRuntime/N64Recomp/lib/fmt checkout 407c905e`.
   Upstream N64Recomp made the same change in `81213c1`.
4. Configure. The extra C++ flags work around clang 22 and newer libstdc++ headers in
   RT64's imgui and RmlUi:

   ```sh
   cmake -S . -B build-cmake -G Ninja -DCMAKE_BUILD_TYPE=Release \
     -DCMAKE_C_COMPILER=clang -DCMAKE_CXX_COMPILER=clang++ \
     -DCMAKE_CXX_FLAGS="-Wno-error=uninitialized-const-pointer -include cstdint" \
     -DRECOMP_RUST_RUNTIME=ON
   cmake --build build-cmake --target Zelda64Recompiled -j6
   ```

   `-DRECOMP_RUST_RUNTIME=OFF` gives the unmodified C++ runtime.

## Checking which implementation is linked

```sh
nm build-cmake/Zelda64Recompiled | grep -E ' (osGetMemSize_recomp|recomp_abi_install_guest_memory)$'
```

With the Rust runtime, both symbols are present, and `osGetMemSize_recomp` calls
`recomp_abi::export::enter` before writing `$v0`.
