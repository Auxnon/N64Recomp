//! Replays frames saved with `RECOMP_RENDER_DUMP` (see `src/dump.rs`) through fast3d,
//! headless, and writes what the VI would show to `frame<N>.png` in the dump directory.
//!
//! ```text
//! cargo run -p recomp-render --example replay -- <dump dir> [scale]
//! ```
//!
//! Frames are replayed in order through one renderer, so framebuffers carry over between
//! them as they do in the game. Each frame also gets `frame<N>_last.png`: the last
//! framebuffer drawn, whatever the VI registers say.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU32;

use fast3d::{ClearPolicy, PresentTarget, Renderer, RendererConfig};
use recomp_abi::GuestMemory;
use recomp_render::context::{GfxTask, MicrocodeCache, RecompHardware};
use recomp_render::diag::DiagLog;
use recomp_render::vi::ViRegs;

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

struct Frame {
    number: u64,
    tasks: Vec<(u32, GfxTask)>,
    vi: Option<ViRegs>,
}

fn hex(token: &str) -> u32 {
    let token = token.trim_start_matches("0x");
    u32::from_str_radix(token, 16).unwrap_or_else(|_| panic!("bad number {token}"))
}

fn read_frame(dir: &Path, number: u64) -> Frame {
    let text = fs::read_to_string(dir.join(format!("frame{number}.txt"))).unwrap();
    let mut frame = Frame {
        number,
        tasks: Vec::new(),
        vi: None,
    };
    for line in text.lines() {
        let words: Vec<&str> = line.split_whitespace().collect();
        match words.first() {
            Some(&"task") => frame.tasks.push((
                words[1].parse().unwrap(),
                GfxTask {
                    ucode: hex(words[2]),
                    ucode_data: hex(words[3]),
                    ucode_data_size: hex(words[4]),
                    data_ptr: hex(words[5]),
                    data_size: hex(words[6]),
                    ..Default::default()
                },
            )),
            Some(&"vi") => {
                let fields: Vec<u32> = words[1..].iter().map(|w| hex(w)).collect();
                frame.vi = Some(ViRegs::from_fields(fields.try_into().unwrap()));
            }
            _ => {}
        }
    }
    frame
}

fn load_rdram(path: &Path) -> Vec<AtomicU32> {
    let bytes = fs::read(path).unwrap();
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|word| AtomicU32::new(u32::from_be_bytes(*word)))
        .collect()
}

fn frames(dir: &Path) -> Vec<u64> {
    let mut numbers: Vec<u64> = fs::read_dir(dir)
        .unwrap()
        .filter_map(|entry| {
            let name = entry.ok()?.file_name().into_string().ok()?;
            name.strip_prefix("frame")?
                .strip_suffix(".txt")?
                .parse()
                .ok()
        })
        .collect();
    numbers.sort();
    numbers
}

fn save_png(path: &PathBuf, width: u32, height: u32, rgba: &[u8]) {
    let file = fs::File::create(path).unwrap();
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .unwrap()
        .write_image_data(rgba)
        .unwrap();
    println!("wrote {}", path.display());
}

/// Renders into a fresh texture with `draw` and reads it back as tightly packed RGBA.
fn read_back(
    renderer: &mut Renderer,
    width: u32,
    height: u32,
    draw: impl FnOnce(&mut Renderer, &wgpu::TextureView),
) -> Vec<u8> {
    let device = renderer.device().clone();
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("replay"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    draw(renderer, &texture.create_view(&Default::default()));

    let row = (width * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("replay readback"),
        size: u64::from(row * height),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    renderer.queue().submit(Some(encoder.finish()));
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, |r| r.unwrap());
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let mapped = buffer.slice(..).get_mapped_range();
    let mut out = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        let start = (y * row) as usize;
        out.extend_from_slice(&mapped[start..start + (width * 4) as usize]);
    }
    // The VI shows color only (the game window asks for an opaque surface), so drop alpha.
    for pixel in out.as_chunks_mut::<4>().0 {
        pixel[3] = 0xFF;
    }
    out
}

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = PathBuf::from(args.next().expect("usage: replay <dump dir> [scale]"));
    let scale: u32 = args.next().map_or(2, |s| s.parse().unwrap());
    let (width, height) = (320 * scale, 240 * scale);
    recomp_render::diag::install_logger();

    let instance = wgpu::Instance::default();
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        ..Default::default()
    }))
    .expect("no adapter");
    println!("adapter: {}", adapter.get_info().name);
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: adapter.features() & fast3d::OPTIONAL_FEATURES,
        ..Default::default()
    }))
    .expect("no device");
    let mut renderer = Renderer::with_device(
        device,
        queue,
        PresentTarget::Headless {
            format: FORMAT,
            width,
            height,
        },
        RendererConfig {
            resolution_multiplier: 1,
            sample_count: 1,
            present_mode: wgpu::PresentMode::Fifo,
            format: Some(FORMAT),
            clear_policy: ClearPolicy::Persist,
            power_preference: wgpu::PowerPreference::HighPerformance,
        },
    );

    let mut microcodes = MicrocodeCache::default();
    let mut diags =
        DiagLog::new(std::env::var_os("RECOMP_RENDER_DIAG").is_some_and(|v| v == "all"));
    for number in frames(&dir) {
        let frame = read_frame(&dir, number);
        let mut last = None;
        for (k, task) in &frame.tasks {
            let words = load_rdram(&dir.join(format!("frame{number}_task{k}.rdram")));
            let mem = GuestMemory::new(&words);
            let Some(ucode) = microcodes.lookup(mem, task) else {
                continue;
            };
            let hardware = RecompHardware { mem, vi: None };
            let summary = renderer.process_dl(
                &hardware,
                u64::from(task.data_ptr & 0x1FFF_FFFF),
                ucode,
                &mut diags,
            );
            println!("frame {number} task {k}: {summary:?}");
            last = Some(words);
        }
        let Some(words) = last else {
            println!("frame {number}: nothing drawn");
            continue;
        };
        let mem = GuestMemory::new(&words);
        let vi = frame.vi.map(|vi| {
            println!("frame {}: {vi:x?} -> {:x?}", frame.number, vi.to_fast3d());
            vi.to_fast3d()
        });
        let hardware = RecompHardware { mem, vi };
        let pixels = read_back(&mut renderer, width, height, |r, view| {
            r.present_to(&hardware, view)
        });
        save_png(
            &dir.join(format!("frame{number}.png")),
            width,
            height,
            &pixels,
        );
        let pixels = read_back(&mut renderer, width, height, |r, view| {
            r.present_last_to(view)
        });
        save_png(
            &dir.join(format!("frame{number}_last.png")),
            width,
            height,
            &pixels,
        );
        renderer.begin_frame();
    }
    diags.summarize();
}
