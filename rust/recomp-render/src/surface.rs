//! Creating a wgpu surface from the game window's native handles.
//!
//! This is item 4 of the unsafe budget in `docs/rust-port-plan.md`. The C++ side gets the
//! handles from `SDL_GetWindowWMInfo` and passes them across as plain values.

use std::ffi::c_void;
use std::num::NonZeroU32;
use std::ptr::NonNull;

use raw_window_handle::{
    RawDisplayHandle, RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle, XcbDisplayHandle,
    XcbWindowHandle, XlibDisplayHandle, XlibWindowHandle,
};

/// Which window system the handles belong to. Values match `RECOMP_RENDER_WINDOW_*` in
/// `include/recomp_render.h`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowSystem {
    /// `display` is an Xlib `Display*`, `window` an Xlib `Window`.
    Xlib,
    /// `display` is a `wl_display*`, `window` a `wl_surface*`.
    Wayland,
    /// `display` is an `xcb_connection_t*`, `window` an `xcb_window_t`.
    Xcb,
}

impl WindowSystem {
    pub fn from_c(kind: u32) -> Option<Self> {
        match kind {
            1 => Some(Self::Xlib),
            2 => Some(Self::Wayland),
            3 => Some(Self::Xcb),
            _ => None,
        }
    }
}

/// A window to draw into.
#[derive(Clone, Copy, Debug)]
pub struct NativeWindow {
    pub system: WindowSystem,
    pub display: *mut c_void,
    pub window: u64,
}

impl NativeWindow {
    fn handles(self) -> Result<(RawDisplayHandle, RawWindowHandle), String> {
        let display = NonNull::new(self.display).ok_or("null display handle")?;
        Ok(match self.system {
            WindowSystem::Xlib => (
                XlibDisplayHandle::new(Some(display), 0).into(),
                XlibWindowHandle::new(self.window as _).into(),
            ),
            WindowSystem::Wayland => {
                let surface =
                    NonNull::new(self.window as usize as *mut c_void).ok_or("null wl_surface")?;
                (
                    WaylandDisplayHandle::new(display).into(),
                    WaylandWindowHandle::new(surface).into(),
                )
            }
            WindowSystem::Xcb => {
                let window = u32::try_from(self.window)
                    .ok()
                    .and_then(NonZeroU32::new)
                    .ok_or("invalid xcb window")?;
                (
                    XcbDisplayHandle::new(Some(display), 0).into(),
                    XcbWindowHandle::new(window).into(),
                )
            }
        })
    }
}

/// Creates a surface for `window`.
///
/// # Safety
///
/// The handles must name a live window and display, and both must outlive the surface.
#[allow(unsafe_code)]
pub unsafe fn create_surface(
    instance: &wgpu::Instance,
    window: NativeWindow,
) -> Result<wgpu::Surface<'static>, String> {
    let (raw_display_handle, raw_window_handle) = window.handles()?;
    let target = wgpu::SurfaceTargetUnsafe::RawHandle {
        raw_display_handle: Some(raw_display_handle),
        raw_window_handle,
    };
    // SAFETY: forwarded from this function's contract.
    unsafe { instance.create_surface_unsafe(target) }.map_err(|e| e.to_string())
}
