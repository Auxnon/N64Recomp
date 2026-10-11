# Rust port plan: N64Recomp + fast3d-rs

Written 2026-10-10. Goal: move the recomp stack to Rust one piece at a time, with each piece
proven to work before the next starts. Rust and C/C++ link together through `extern "C"`
until the last C++ module is gone. Graphics use wgpu through
[fast3d-rs](https://github.com/retrofoundry/fast3d-rs). `unsafe` lives in a short, named
list of places, and every other crate uses `#![forbid(unsafe_code)]`.

## Status

- **Phase 0 (in progress):** Zelda64Recomp `b65c482` (MM US) is cloned next to this repo.
  - Building it with clang 22 needed two local fixes:
    - Its pinned N64Recomp's `fmt` was moved to fmt 12.1.0, same as upstream's `81213c1`.
    - `patches/Makefile` got `-Wno-error=incompatible-pointer-types`.
  - **Golden recompiler output recorded:** `rust/golden/golden.sh` runs a given
    N64Recomp/RSPRecomp pair on MM's four configs (`us.rev1`, `aspMain`, `njpgdspMain`,
    `patches`) in a scratch directory. It writes `zelda64recomp-mm-us.sha256` (348 files, hashes
    only) or checks against it. The C++ tools reproduce it exactly, and it's the output
    Zelda64Recomp's build compiles. This is the Phase 4 exit check.
  - Microcodes in the MM ROM: **F3DZEX.NoN fifo 2.08I** (F3DZEX2, no near clipping) and
    **S2DEX fifo 2.08**. fast3d has neither.
  - RT64 extended GBI used by MM's patches, by call count:
    - Matrix groups (interpolation): `gEXMatrixGroup*`, `gEXPopMatrixGroup`,
      `gEXEditGroupByAddress`.
    - Widescreen alignment: `gEXSetViewportAlign`, `gEXSetRectAlign`, `gEXSetScissorAlign`.
    - Push/pop state: `gEXPush/Pop{GeometryMode,OtherMode,CombineMode,Viewport,ProjectionMatrix}`.
    - Extended draws: `gEXVertex`, `gEXTextureRectangle`, `gEXFillRectangle`.
    - Other: `gEXVertexZTest`, `gEXEndVertexZTest`, `gEXForceUpscale2D`, `gEXSetRefreshRate`,
      `gEXSetRDRAMExtended`, `gEXSetDitherNoiseStrength`, `gEXEnable`.
  - The Gauntlet ROM has no "RSP Gfx ucode" string, possibly because it is compressed or
    custom. A separate effort is documenting it.
- **Phase 1:** `rust/recomp-abi` is written and tested, and `rust/recomp-runtime` replaces
  `osGetMemSize_recomp` in Zelda64Recomp (see `rust/integration/README.md`).
  - The symbol check confirms the binary links the Rust version.
  - **Exit check passed (2026-10-11):** the `-DRECOMP_RUST_RUNTIME=ON` build boots on the
    desktop and starts a game. MM checks the reported memory size at boot, so a wrong value
    would have failed early.
  - `RecompContext` / `Fpr` layout is checked against `recomp.h` compiled by the C compiler.
  - `GuestMemory` matches `MEM_*`, `LD`/`SD` and LWL/LWR/SWL/SWR/LDL/LDR/SDL/SDR over
    200k random operations, compared against `recomp.h` (`rust/recomp-abi-ctest`).
  - A `recomp_export!` / `GameFn` round trip works: Rust calls C, and C calls Rust.

## Target games

1. **Zelda 64 first: proven ground.** [Zelda64Recomp](https://github.com/Zelda64Recomp/Zelda64Recomp)
   ships Majora's Mask on this exact stack, and its README says Ocarina of Time support is
   coming. Use MM until the OoT recomp is public, then add OoT. Known costs for fast3d:
   - Both games use **F3DZEX2**, which fast3d doesn't have yet. It's close to F3DEX2.
   - Zelda64Recomp's patches emit **RT64 extended GBI** commands (matrix groups for
     interpolation, widescreen origins, and so on). fast3d has to at least skip these safely.
   - The user supplies the ROM. Nothing ROM-derived goes in this repo.
2. **Gauntlet Legends second:** the `../gauntlet-legends-recomp` spike. It boots through
   N64Recomp + N64ModernRuntime + RT64, runs mapped code at `0xE0xxxxxx` (TLB emulation in
   `src/tlb.cpp`) and has no audio yet. Its graphics microcode may be custom; a separate
   effort is documenting it.

## Starting points

| Piece | Today | Rust source |
|---|---|---|
| Renderer | RT64 behind `ultramodern::renderer::RendererContext` (`rt64_context.cpp` in the spike) | fast3d-rs (MIT) plus an adapter crate |
| Guest ABI (`recomp_context`, `MEM_*`, LWL/LWR helpers) | `include/recomp.h` | Port `recomp.h` directly. Reuse the *design* of `retrofoundry/n64-recomp-rs` (`#[repr(C)]` context, arg/return helpers, `N64MemoryIO` derive) but **not its code: that repo has no license.** |
| Recompiler tool (~14k LoC C++) | this repo | Port in this repo |
| Runtime (ultramodern + librecomp) | N64ModernRuntime (GPL-3) | Port last, module by module. The Rust port stays GPL-3. |

## What fast3d-rs gives us, and what's missing for recomp

Already there: F3D and F3DEX2 HLE, color combiner, blender, GPU TMEM decode with texture
residency, persistent depth, framebuffer aliases for load-free TexRects, VI scanout
(`ViRegisters`), `ClearPolicy::Persist` for live games, `RenderHook` for overlay UI (egui),
capture/replay, an **rt64-oracle** tool that compares fast3d frames with RT64, structured
diagnostics (`DiagSink`), and a safe public `Rdram` trait we can implement ourselves.

Gaps, in the order we'll probably hit them:

1. **Memory layout.** `RdramImage` reads big-endian bytes at physical addresses. Recomp RDRAM
   stores native-endian 32-bit words (the `^3` / `^2` swizzle in `recomp.h`) and is addressed
   through KSEG0 (`0x80…`). Gauntlet also uses TLB addresses (`0xE0…`). Fix: our own
   `Rdram` impl. No fast3d fork needed.
2. **Microcode selection.** The caller has to pick the microcode, and only `F3d` and `F3dex2`
   exist. We have to identify it from the `OSTask` (the "RSP Gfx ucode …" version string in
   `ucode_data`, or a hash of `ucode`). We don't know Gauntlet's microcode yet. F3DEX, F3DLX,
   F3DZEX2 and S2DEX are all missing.
3. `resolution_multiplier` and MSAA are clamped to 1 in v1.
4. No CPU/GPU framebuffer coherence or write-back to RDRAM. Games that read the framebuffer
   on the CPU will draw wrong.
5. Combiner inputs `KEY_CENTER`, `KEY_SCALE`, K4 and K5 aren't wired up, and TEXTCONV other than
   `G_TC_FILT` and chroma key are rejected. Affected draws are dropped with an Error diagnostic.
6. No widescreen, no frame interpolation, no RT64 extended GBI. Gauntlet doesn't need these to
   be playable. Leave them until a target game does.

## Layout

A Cargo workspace in this repo under `rust/`:

- `recomp-abi`: `RecompContext`/`Fpr` (`#[repr(C)]`), `GuestMemory`, the `recomp_export!`
  macro, `GameFn`. Holds nearly all of the unsafe code.
- `recomp-render`: fast3d adapter. `RecompRdram: fast3d::Rdram`, microcode detection, VI
  register mapping, and a C ABI the C++ `RendererContext` shim calls into.
- `recomp-core` + binaries `n64recomp`, `rsp-recomp`, then `mod-tool`, `mod-merger`,
  `offline-mod-recomp` and `live-recomp` (cranelift in place of sljit).
- Later: `ultramodern`, `librecomp`.

Which side hosts the build:
- **First:** the C++ CMake app stays the host and links Rust static libraries through
  [Corrosion](https://github.com/corrosion-rs/corrosion). `cbindgen` writes the headers.
- **Finally (end of phase 5):** Cargo hosts the build and compiles the generated C with the
  `cc` crate. CMake goes away.

### Guest memory

- `GuestMemory` is created once from the runtime's mapping. Inside, it's
  `&'static [AtomicU32]`. Creating that slice is one `unsafe` block. The mapping lives as long
  as the process and is word-aligned.
- Byte and halfword accessors apply the same `^3`/`^2` swizzle as `recomp.h`. Relaxed atomics
  compile to plain loads and stores on x86-64 and ARM64.
- **Why atomics:** C game threads, the graphics thread (Rust) and audio all touch the same
  memory at once. A `&[u8]` or `&mut [u8]` over it would be undefined behavior in Rust.
- **Remaining caveat:** the C side's stores are still plain stores. That cross-language race
  is the same one every emulator has, and it's documented in one place.
- **Gauntlet's TLB:** its `0xE0…` pages are memfd aliases of RDRAM inside the 4 GB reservation.
  `RecompRdram` translates TLB addresses to physical offsets using the same table `tlb.cpp`
  keeps, instead of relying on the aliases. This translation moves to Rust when `tlb.cpp` does.
- Allocation for reference: 512 MB of KSEG0 committed inside a 4 GB reservation. Mod RAM is at
  `0x81000000`. **Masking addresses to 8 MB would be wrong.**

### The unsafe budget (the complete list)

1. Creating the `GuestMemory` view from the runtime's pointer.
2. `recomp_export!`: turning `(*mut u8, *mut recomp_context)` from game code into
   `(&GuestMemory, &mut RecompContext)`. The body of every exported function is safe code.
3. `GameFn::call`: calling an `unsafe extern "C" fn(*mut u8, *mut recomp_context)`
   (`LOOKUP_FUNC` targets, thread entry points).
4. Creating a wgpu surface from the SDL window's raw handles.

CI fails if `unsafe` appears outside these modules (a grep check plus `forbid` attributes).

## Phases

Every phase has an exit check. Don't start the next phase until it passes.

### Phase 0: baselines (days)

- Build Zelda64Recomp (MM) unchanged with RT64.
- **Golden recompiler output.** Run the current C++ `N64Recomp` and `RSPRecomp` on
  Zelda64Recomp's configs (`us.rev1.toml`, `patches.toml`, the aspMain/njpgdspMain RSP configs).
  Then do the same for Gauntlet's `gauntlet.us.toml`. Record hashes of everything they write.
- **Golden frames.** Pick a few deterministic points (boot logo, title, character select,
  first level) and save RT64 screenshots plus fast3d captures of those frames.
- **Microcode inventory.** In each game's `send_dl`, log the "RSP Gfx ucode …" version
  string from `task->t.ucode_data`. MM should report F3DZEX2. Gauntlet's is being documented
  separately (it may be custom).
- **RT64 extended GBI inventory.** List which `gEX*` commands MM's patches emit.
- **Exit:** the golden corpus reproduces and both inventories are written down.

### Phase 1: `recomp-abi` (1–2 weeks)

- Port the context and memory semantics from `recomp.h`: `MEM_W/H/B/HU/BU`, `SD`,
  `load_doubleword`, LWL/LWR/SWL/SWR, LDL/LDR/SDL/SDR, `DMULT`/`DDIV`.
- Build `GuestMemory`, `recomp_export!` and `GameFn`.
- Tests:
  - Layout: compile a C file that prints `offsetof`/`sizeof` for `recomp_context` (built with
    `cc` in `build.rs`) and assert they match the Rust struct.
  - Memory: property tests that run each accessor in Rust and in a C reference, on the same
    buffer.
- **Exit:** one runtime function in the spike, a small stub from `game_funcs.c` or
  `osGetMemSize_recomp`, is replaced with a Rust export, and the game still boots.

### Phase 2: fast3d inside Zelda64Recomp (MM), then Gauntlet

- **2a, prerequisites in fast3d-rs, sent upstream:**
  - Add F3DZEX2 (the NoN variant MM uses), using `hle/rsp_f3dex2.rs` as the template.
  - Add S2DEX (MM uses it for 2D backgrounds and sprites).
  - Handle the RT64 extended GBI commands MM uses (opcode `0x64`, `rt64_extended_gbi.h`):
    - **Can be no-ops at first:** matrix groups, `EditGroupByAddress`, the alignment setters,
      `SetRefreshRate`, `ForceUpscale2D`, `SetDitherNoiseStrength`, `SetRDRAMExtended`,
      `Enable`. Without them we only lose interpolation and widescreen.
    - **Need real implementations:**
      - `gEXVertex`, `gEXTextureRectangle` and `gEXFillRectangle` draw geometry.
      - The `Push`/`Pop` state commands change render state.
      - `VertexZTest` / `EndVertexZTest` changes which geometry draws.
      Treating any of these as no-ops would drop or corrupt geometry.

- `recomp-render`:
  - `RecompRdram` handles the swizzle, KSEG0/TLB translation and segment table.
  - Microcode detection from the `OSTask`.
  - A `ViRegs` → `fast3d::ViRegisters` mapping.
  - The `Renderer` is owned by the graphics thread.
- C++ side: add a `fast3d_context.cpp` next to each game's RT64 context
  (Zelda64Recomp's `src/main/rt64_render_context.cpp`, the spike's `src/rt64_context.cpp`). It implements
  `RendererContext` and forwards `send_dl`, `update_screen`, `send_dummy_workload`,
  `update_config`, `get_display_framerate` and `get_resolution_scale` (returns 1) to Rust.
  A CLI flag or env var picks the renderer, so RT64 stays available as the reference.
- Window: build a wgpu surface from the SDL window, on XWayland as now.
- Validate:
  - Run with `LogSink`, play from the title screen into gameplay (MM: title → Clock Town),
    and collect every
    `Unsupported*` or `MemoryRead` diagnostic. That list becomes the Phase 3 backlog.
  - Compare the golden frames using fast3d capture + `tools/rt64-oracle`.
- **Exit (MM):** the title screen and Clock Town render with no Error diagnostics, and the
  golden frames match RT64 within tolerance.
- **Then Gauntlet:** add its microcode, whatever the separate documentation effort finds,
  and repeat the same checks.

### Phase 3: closing fast3d's gaps, upstream first (ongoing)

- Order the work by the games' diagnostic lists, not by guesses. Likely candidates:
  - framebuffer write-back
  - `resolution_multiplier` > 1
  - the unwired combiner inputs
- Every fix comes with an rt64-oracle fixture.
- Talk to retrofoundry (their Discord) before large features, and keep our local patch stack
  small.

### Phase 4: the recompiler tool (alongside phases 2–3, 1–2 months)

Each step has to produce **byte-identical** output against the phase 0 corpus.

1. `recomp-core` foundations:
   - `operations.{h,cpp}` tables
   - `context.h` types
   - `config.cpp` (`toml` + `serde`)
   - `symbol_lists.cpp`
   - `elf.cpp` and the symbol-file loader (`object` crate)
   - `mdebug.{h,cpp}`
2. `analysis.cpp` (jump tables), `recompilation.cpp`, and `Generator` as a trait with
   `CGenerator` as its first implementation.
3. The `n64recomp` binary (`src/main.cpp`). **Exit:** output for MM's configs and
   `gauntlet.us.toml` is identical.
4. `rsp-recomp` (`RSPRecomp/src/rsp_recomp.cpp`). Check it against MM's RSP outputs. It also
   unblocks Gauntlet's audio (`aspMain`).
5. `RecompModTool`, `RecompModMerger`, `OfflineModRecomp`, only once mods are in scope.
6. `LiveRecomp` on cranelift, last.

Notes:
- Use the `rabbitizer` crate, pinned to the version that matches the `lib/rabbitizer`
  submodule (`e0d8003`), so instruction decoding doesn't drift.
- Upstream N64Recomp keeps changing. Port each upstream commit and add a golden case for it.

### Phase 5: the runtime (months, one module at a time)

- Replace N64ModernRuntime's C++ one file at a time. Each Rust module exports the
  **same `extern "C"` symbols** (`osX_recomp(uint8_t* rdram, recomp_context* ctx)`), so the
  generated C and the remaining C++ link unchanged. Order:
  1. Leaves: `math_routines`, `print`, `euc-jp`, `ultra_stubs`.
  2. Saves: `eep`, `pak`, `flash`.
  3. PI/DMA, plus the spike's `boot.cpp` and `tlb.cpp`.
  4. The scheduler core: `mesgqueue`, `timer`, `events`, `threads`, `scheduling`.
     Keep the one-game-thread-at-a-time semantics exactly.
  5. Audio, input, config.
  6. Mods last.
- **Exit per module:** MM (then Gauntlet) boots and the golden frames still match.
- At the end, switch the build so Cargo hosts it (see Layout).

### Phase 6 (optional): generating Rust instead of C

- Only after phases 1–5. Add a `RustGenerator` next to `CGenerator`:
  - `loop { match block }` in place of `goto`
  - `GuestMemory` accessors for memory
  - explicit FP rounding in place of `fenv`
  - a function table in place of weak-symbol patching
- Measure build time and frame time on MM before going any further.

## Open decisions

1. **Where the runtime port lives:** this repo, a fork of N64ModernRuntime, or a new repo.
   Runtime code stays GPL-3 either way. The tool and render crates can stay MIT.
2. **fast3d-rs:** contribute upstream (preferred) or keep a long-lived fork.
3. **n64-recomp-rs:** ask retrofoundry to add a license if we want to use its code directly.
   Otherwise write our own from `recomp.h`; it's under 1k lines.
