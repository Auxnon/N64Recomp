//! fast3d-rs as the renderer for a recompiled game (docs/rust-port-plan.md, phase 2b).
//!
//! The C++ game keeps ultramodern's `RendererContext` interface. A small C++ class
//! (`rust/integration/fast3d_render_context.cpp`) implements it by calling the
//! `recomp_render_fast3d_*` functions here, declared in `include/recomp_render.h`.
//!
//! - [`rdram::RecompRdram`] reads recomp RDRAM (native-endian words, KSEG0 base) for fast3d.
//! - [`vi::ViRegs`] maps ultramodern's VI registers to fast3d's scanout.
//! - [`context::Fast3dContext`] owns the fast3d `Renderer` and identifies each task's
//!   microcode from its data segment.
//!
//! ultramodern creates the renderer on its graphics thread and makes every call from that
//! thread, so the context lives in a thread-local. A call from any other thread finds no
//! renderer and does nothing.

#![deny(unsafe_code)]

pub mod context;
pub mod diag;
pub mod dump;
pub mod rdram;
pub mod surface;
pub mod vi;

use std::cell::RefCell;
use std::ffi::c_void;

use context::{Fast3dContext, GfxTask};
use surface::{NativeWindow, WindowSystem};
use vi::ViRegs;

thread_local! {
    static CONTEXT: RefCell<Option<Fast3dContext>> = const { RefCell::new(None) };
}

fn create(
    system: u32,
    display: *mut c_void,
    window: u64,
    width: u32,
    height: u32,
) -> Result<(), String> {
    diag::install_logger();
    let mem = recomp_abi::guest_memory()
        .ok_or("RDRAM isn't registered (recomp_abi_install_guest_memory)")?;
    let system = WindowSystem::from_c(system).ok_or(format!("unknown window system {system}"))?;
    let instance = wgpu::Instance::default();
    #[allow(unsafe_code)]
    // SAFETY: the caller of `recomp_render_fast3d_create` guarantees the handles.
    let surface = unsafe {
        surface::create_surface(
            &instance,
            NativeWindow {
                system,
                display,
                window,
            },
        )?
    };
    let context = Fast3dContext::new(instance, surface, width, height, *mem)?;
    CONTEXT.with_borrow_mut(|slot| *slot = Some(context));
    Ok(())
}

/// Creates the renderer for the calling (graphics) thread. Returns `false` and prints why on
/// failure.
///
/// # Safety
///
/// `display` and `window` must be live handles of the kind `system` names
/// (`RECOMP_RENDER_WINDOW_*`), and stay valid until [`recomp_render_fast3d_shutdown`].
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn recomp_render_fast3d_create(
    system: u32,
    display: *mut c_void,
    window: u64,
    width: u32,
    height: u32,
) -> bool {
    match create(system, display, window, width, height) {
        Ok(()) => true,
        Err(e) => {
            eprintln!("[fast3d] renderer setup failed: {e}");
            false
        }
    }
}

/// Draws one graphics task's display list.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn recomp_render_fast3d_send_dl(task: GfxTask) {
    CONTEXT.with_borrow_mut(|slot| {
        if let Some(context) = slot {
            context.send_dl(&task);
        }
    });
}

/// Scans out the framebuffer the VI registers select, at the window's current pixel size.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn recomp_render_fast3d_update_screen(vi: ViRegs, width: u32, height: u32) {
    CONTEXT.with_borrow_mut(|slot| {
        if let Some(context) = slot {
            context.update_screen(vi, width, height);
        }
    });
}

/// Waits for the GPU and destroys the renderer.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn recomp_render_fast3d_shutdown() {
    if let Some(context) = CONTEXT.with_borrow_mut(Option::take) {
        context.shutdown();
    }
}
