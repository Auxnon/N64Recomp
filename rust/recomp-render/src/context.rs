//! The renderer the graphics thread owns: what ultramodern's `RendererContext` forwards to.

use std::collections::HashMap;

use fast3d::{
    ClearPolicy, Hardware, Microcode, PresentError, PresentTarget, Rdram, Renderer, RendererConfig,
    ViRegisters, detect_microcode_from_data,
};
use recomp_abi::GuestMemory;

use crate::diag::DiagLog;
use crate::dump::FrameDump;
use crate::rdram::RecompRdram;
use crate::vi::ViRegs;

/// How often (in presented frames) to print the diagnostic counts.
const SUMMARY_INTERVAL: u64 = 600;
/// The most of `ucode_data` read when looking for the version string.
const UCODE_DATA_LIMIT: u32 = 0x1000;

/// The graphics fields of an `OSTask`, as guest addresses.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GfxTask {
    pub ucode: u32,
    pub ucode_size: u32,
    pub ucode_data: u32,
    pub ucode_data_size: u32,
    pub data_ptr: u32,
    pub data_size: u32,
}

/// A KSEG0/KSEG1 task pointer as an RDRAM offset.
fn task_address(address: u32) -> u64 {
    u64::from(address & 0x1FFF_FFFF)
}

/// Guest memory plus the VI registers for one call into fast3d.
pub struct RecompHardware<'m> {
    pub mem: GuestMemory<'m>,
    pub vi: Option<ViRegisters>,
}

impl Hardware for RecompHardware<'_> {
    fn rdram(&self) -> impl Rdram + '_ {
        RecompRdram::new(self.mem)
    }

    fn vi(&self) -> Option<ViRegisters> {
        self.vi
    }
}

/// Identifies microcodes by their data segment, remembering each one by address.
#[derive(Default)]
pub struct MicrocodeCache {
    known: HashMap<(u32, u32), Option<Microcode>>,
}

impl MicrocodeCache {
    pub fn lookup(&mut self, mem: GuestMemory<'_>, task: &GfxTask) -> Option<Microcode> {
        *self.known.entry((task.ucode, task.ucode_data)).or_insert_with(|| {
            let length = task.ucode_data_size.min(UCODE_DATA_LIMIT) as usize;
            let rdram = RecompRdram::new(mem);
            let found = rdram
                .read_bytes(task_address(task.ucode_data), length)
                .ok()
                .and_then(|data| detect_microcode_from_data(&data));
            match found {
                Some(ucode) => eprintln!(
                    "[fast3d] microcode at {:#010x} (data {:#010x}): {ucode:?}",
                    task.ucode, task.ucode_data
                ),
                None => eprintln!(
                    "[fast3d] unsupported microcode at {:#010x} (data {:#010x}); its display lists won't be drawn",
                    task.ucode, task.ucode_data
                ),
            }
            found
        })
    }
}

/// Picks the swapchain format: a non-sRGB format if offered, else an sRGB one viewed as
/// linear, so colors aren't gamma-encoded twice (fast3d's `pick_surface_format`).
fn surface_formats(
    available: &[wgpu::TextureFormat],
) -> (wgpu::TextureFormat, Vec<wgpu::TextureFormat>) {
    if let Some(&format) = available.iter().find(|f| !f.is_srgb()) {
        return (format, Vec::new());
    }
    let format = available
        .first()
        .copied()
        .unwrap_or(wgpu::TextureFormat::Bgra8Unorm);
    (format, vec![format.remove_srgb_suffix()])
}

/// The game paces itself on VI interrupts, so presenting shouldn't block on vsync.
fn present_mode(available: &[wgpu::PresentMode]) -> wgpu::PresentMode {
    [wgpu::PresentMode::Mailbox, wgpu::PresentMode::Immediate]
        .into_iter()
        .find(|mode| available.contains(mode))
        .unwrap_or(wgpu::PresentMode::Fifo)
}

/// The display has no alpha, so ask the compositor to ignore it. `Auto` can pick a
/// premultiplied mode on Wayland, where any alpha below one shows what's behind the window.
fn alpha_mode(available: &[wgpu::CompositeAlphaMode]) -> wgpu::CompositeAlphaMode {
    if available.contains(&wgpu::CompositeAlphaMode::Opaque) {
        wgpu::CompositeAlphaMode::Opaque
    } else {
        wgpu::CompositeAlphaMode::Auto
    }
}

pub struct Fast3dContext {
    // Dropped in this order: the renderer (and its surface) before the instance.
    renderer: Renderer,
    _instance: wgpu::Instance,
    mem: GuestMemory<'static>,
    microcodes: MicrocodeCache,
    diags: DiagLog,
    size: (u32, u32),
    frames: u64,
    dump: Option<FrameDump>,
}

impl Fast3dContext {
    pub fn new(
        instance: wgpu::Instance,
        surface: wgpu::Surface<'static>,
        width: u32,
        height: u32,
        mem: GuestMemory<'static>,
    ) -> Result<Self, String> {
        let power_preference = wgpu::PowerPreference::HighPerformance;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference,
            force_fallback_adapter: false,
            compatible_surface: Some(&surface),
        }))
        .map_err(|e| format!("no graphics adapter: {e}"))?;
        let info = adapter.get_info();
        eprintln!("[fast3d] adapter: {} ({:?})", info.name, info.backend);

        let required_features = adapter.features() & wgpu::Features::DUAL_SOURCE_BLENDING;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("recomp-render"),
            required_features,
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::default(),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            trace: wgpu::Trace::Off,
        }))
        .map_err(|e| format!("device request failed: {e}"))?;

        let caps = surface.get_capabilities(&adapter);
        let (format, view_formats) = surface_formats(&caps.formats);
        let present_mode = present_mode(&caps.present_modes);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: width.max(1),
            height: height.max(1),
            present_mode,
            alpha_mode: alpha_mode(&caps.alpha_modes),
            view_formats,
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let renderer = Renderer::with_device(
            device,
            queue,
            PresentTarget::Surface { surface, config },
            RendererConfig {
                resolution_multiplier: 1,
                sample_count: 1,
                present_mode,
                format: None,
                clear_policy: ClearPolicy::Persist,
                power_preference,
            },
        );
        let verbose = std::env::var_os("RECOMP_RENDER_DIAG").is_some_and(|v| v == "all");
        Ok(Self {
            renderer,
            _instance: instance,
            mem,
            microcodes: MicrocodeCache::default(),
            diags: DiagLog::new(verbose),
            size: (width, height),
            frames: 0,
            dump: FrameDump::from_env(),
        })
    }

    pub fn send_dl(&mut self, task: &GfxTask) {
        if let Some(dump) = &mut self.dump {
            dump.task(self.frames, self.mem, task);
        }
        let Some(ucode) = self.microcodes.lookup(self.mem, task) else {
            return;
        };
        let hardware = RecompHardware {
            mem: self.mem,
            vi: None,
        };
        self.renderer.process_dl(
            &hardware,
            task_address(task.data_ptr),
            ucode,
            &mut self.diags,
        );
    }

    pub fn update_screen(&mut self, vi: ViRegs, width: u32, height: u32) {
        if (width, height) != self.size && width > 0 && height > 0 {
            self.renderer.resize(width, height);
            self.size = (width, height);
        }
        let hardware = RecompHardware {
            mem: self.mem,
            vi: Some(vi.to_fast3d()),
        };
        if let Some(dump) = &mut self.dump {
            dump.present(self.frames, &vi);
        }
        match self.renderer.present(&hardware) {
            Ok(()) | Err(PresentError::SurfaceLost | PresentError::Timeout) => {}
            Err(e) => eprintln!("[fast3d] present failed: {e:?}"),
        }
        self.renderer.begin_frame();
        self.frames += 1;
        if self.frames.is_multiple_of(SUMMARY_INTERVAL) {
            self.diags.summarize();
        }
    }

    pub fn shutdown(self) {
        let Self {
            renderer,
            mut diags,
            ..
        } = self;
        diags.summarize();
        renderer.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicU32;

    use recomp_abi::RDRAM_BASE;

    use super::*;

    #[test]
    fn microcode_is_found_by_its_version_string_and_cached() {
        let words: Vec<AtomicU32> = (0..0x200).map(|_| AtomicU32::new(0)).collect();
        let mem = GuestMemory::new(&words);
        mem.write_bytes(
            RDRAM_BASE + 0x100,
            b"RSP Gfx ucode F3DZEX.NoN  fifo 2.08I Yoshitaka Yasumoto 1999 Nintendo.",
        );
        let task = GfxTask {
            ucode: 0x8000_1000,
            ucode_data: 0x8000_0000,
            ucode_data_size: 0x800,
            ..Default::default()
        };
        let mut cache = MicrocodeCache::default();
        assert_eq!(cache.lookup(mem, &task), Some(Microcode::F3dzex2));

        // A second lookup doesn't read memory again.
        mem.write_bytes(RDRAM_BASE + 0x100, &[0; 16]);
        assert_eq!(cache.lookup(mem, &task), Some(Microcode::F3dzex2));

        let unknown = GfxTask {
            ucode_data: 0x8000_0400,
            ..task
        };
        assert_eq!(cache.lookup(mem, &unknown), None);
    }

    #[test]
    fn surface_format_prefers_linear() {
        use wgpu::TextureFormat::*;
        assert_eq!(
            surface_formats(&[Bgra8UnormSrgb, Bgra8Unorm]),
            (Bgra8Unorm, vec![])
        );
        assert_eq!(
            surface_formats(&[Rgba8UnormSrgb]),
            (Rgba8UnormSrgb, vec![Rgba8Unorm])
        );
    }

    #[test]
    fn alpha_mode_is_opaque_when_offered() {
        use wgpu::CompositeAlphaMode::*;
        assert_eq!(alpha_mode(&[PreMultiplied, Opaque]), Opaque);
        assert_eq!(alpha_mode(&[PreMultiplied, Inherit]), Auto);
    }

    #[test]
    fn present_mode_avoids_vsync_when_possible() {
        use wgpu::PresentMode::*;
        assert_eq!(present_mode(&[Fifo, Immediate, Mailbox]), Mailbox);
        assert_eq!(present_mode(&[Fifo, Immediate]), Immediate);
        assert_eq!(present_mode(&[Fifo]), Fifo);
    }
}
