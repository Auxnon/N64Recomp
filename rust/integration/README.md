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
     - With `RECOMP_RUST_FAST3D` as well, adds the fast3d renderer (see below).
     - Guards three UI calls the game makes without the menus (graphics-options refresh,
       fullscreen toggle, mouse activation) and quits directly on window close under fast3d.
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

## The fast3d renderer (phase 2b)

`rust/recomp-render` draws with [fast3d-rs](https://github.com/Auxnon/fast3d-rs/tree/recomp-mm)
in place of RT64. It is built into the Rust runtime library and chosen when the game starts,
so RT64 stays available for comparison.

1. Configure with both options and build as above:

   ```sh
   cmake -S . -B build-cmake ... -DRECOMP_RUST_RUNTIME=ON -DRECOMP_RUST_FAST3D=ON
   cmake --build build-cmake --target Zelda64Recompiled -j6
   ```

   `zelda64recomp.patch` adds `fast3d_render_context.cpp` to the game and makes
   `zelda64::renderer::create_render_context` return it when asked.
2. Run with `RECOMP_RENDERER=fast3d`:

   ```sh
   RECOMP_RENDERER=fast3d ./Zelda64Recompiled
   ```

   Without the variable the game uses RT64 as usual.

What to expect:

- **No menus.** Zelda64Recomp draws its launcher and settings with RmlUi through RT64's
  render hooks, which fast3d doesn't have. With fast3d the game starts on the first screen update, using
  the ROM chosen earlier from the RT64 launcher, so run the RT64 build once first.
- **Output on stderr**, prefixed `[fast3d]`:
  - the adapter and each microcode the game loads;
  - the first three diagnostics of each kind, then a count of every kind every 600 frames
    and at exit. `RECOMP_RENDER_DIAG=all` prints every diagnostic.
  - `RECOMP_RENDER_LOG=info` (or `debug`, `trace`) shows fast3d's own log.
- A display list that faults reading memory is dropped whole, so a black or frozen frame
  with `MemoryRead` diagnostics points at an address problem.
- The window is created by SDL on X11 or Wayland. Graphics settings, resolution scaling,
  widescreen and frame interpolation are ignored.

### Gauntlet Legends

`gauntlet-legends-recomp.patch` does the same for the Gauntlet Legends spike
(`../gauntlet-legends-recomp`, which pins the same N64ModernRuntime commit as Zelda64Recomp):
the two CMake options, and `gauntlet::create_render_context` returning fast3d when asked.

```sh
cd ../gauntlet-legends-recomp
git apply ../n64recomp/rust/integration/gauntlet-legends-recomp.patch
git -C lib/N64ModernRuntime apply ../../../n64recomp/rust/integration/runtime.patch
CC=clang CXX=clang++ cmake -S . -B build-fast3d -G Ninja -DCMAKE_BUILD_TYPE=RelWithDebInfo \
    -DRECOMP_RUST_RUNTIME=ON -DRECOMP_RUST_FAST3D=ON
cmake --build build-fast3d -j6
RECOMP_RENDERER=fast3d ./build-fast3d/GauntletRecompiled ../gauntlet-legends/files/baserom.z64
```

Gauntlet starts itself from its VI callback, so nothing waits for the first screen update.
Its microcode identifies as `F3DEX fifo 2.06`, which fast3d runs as F3DEX2.
