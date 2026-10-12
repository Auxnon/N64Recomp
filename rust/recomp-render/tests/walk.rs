//! fast3d walks a display list in recomp RDRAM exactly as it walks the same list in its own
//! big-endian `RdramImage`, including when the list uses KSEG0 pointers.

use std::ops::ControlFlow;
use std::sync::atomic::AtomicU32;

use fast3d::inspect::{self, WalkObserver, WalkStep};
use fast3d::{DataFormat, Microcode, Rdram, RdramImage};
use n64_gbi::consts::rdp::{G_RM_OPA_SURF, G_RM_OPA_SURF2};
use n64_gbi::encode::*;
use recomp_abi::{GuestMemory, RDRAM_BASE};
use recomp_render::rdram::RecompRdram;

const SIZE: usize = 0x40000;
const MAIN_DL: u32 = 0x1000;
const SUB_DL: u32 = 0x3000;
const DATA: u32 = 0x10000;
const FRAMEBUFFER: u32 = 0x20000;

/// Records every step as text, so two walks can be compared.
#[derive(Default)]
struct Steps(Vec<String>);

impl WalkObserver for Steps {
    fn command(&mut self, step: WalkStep<'_>) -> ControlFlow<()> {
        self.0.push(format!("{step:?}"));
        ControlFlow::Continue(())
    }
}

fn put(bytes: &mut [u8], at: u32, data: &[u8]) {
    bytes[at as usize..at as usize + data.len()].copy_from_slice(data);
}

fn put_commands(bytes: &mut [u8], at: u32, commands: &[(u32, u32)]) {
    for (i, (w0, w1)) in commands.iter().enumerate() {
        let at = at + 8 * i as u32;
        put(bytes, at, &w0.to_be_bytes());
        put(bytes, at + 4, &w1.to_be_bytes());
    }
}

/// A big-endian RDRAM image holding a small F3DZEX2 scene. Pointers in the list are made with
/// `pointer`, so the same scene can use physical or KSEG0 addresses.
fn scene(pointer: impl Fn(u32) -> u32) -> Vec<u8> {
    let mut bytes = vec![0; SIZE];
    // Viewport: scale and translate (320x240), in quarter pixels.
    let viewport: [i16; 8] = [640, 480, 511, 0, 640, 480, 511, 0];
    put(
        &mut bytes,
        DATA,
        &viewport
            .iter()
            .flat_map(|v| v.to_be_bytes())
            .collect::<Vec<_>>(),
    );
    // Identity matrix in N64 fixed point: integer halves, then fraction halves.
    let mut matrix = [0u8; 64];
    for i in 0..4 {
        matrix[(i * 5) * 2 + 1] = 1;
    }
    put(&mut bytes, DATA + 0x40, &matrix);
    // Projection: x, y and z scaled by 1/256 (fraction 0x0100), w kept.
    let mut projection = [0u8; 64];
    for i in 0..3 {
        projection[32 + (i * 5) * 2] = 1;
    }
    projection[(3 * 5) * 2 + 1] = 1;
    put(&mut bytes, DATA + 0x80, &projection);
    // Three shaded vertices.
    let vertices: [([i16; 3], [u8; 4]); 3] = [
        ([-100, -100, 0], [255, 0, 0, 255]),
        ([100, -100, 0], [0, 255, 0, 255]),
        ([0, 100, 0], [0, 0, 255, 255]),
    ];
    for (i, (pos, rgba)) in vertices.iter().enumerate() {
        let at = DATA + 0x100 + 16 * i as u32;
        for (j, p) in pos.iter().enumerate() {
            put(&mut bytes, at + 2 * j as u32, &p.to_be_bytes());
        }
        put(&mut bytes, at + 12, rgba);
    }

    put_commands(
        &mut bytes,
        MAIN_DL,
        &[
            gsp_segment(6, pointer(DATA)),
            gdp_set_color_image(0, 2, 320, pointer(FRAMEBUFFER)),
            gdp_set_scissor(0, 0, 0, 320 << 2, 240 << 2),
            gdp_set_cycle_type(3),
            gdp_set_fill_color(0x0001_0001),
            gdp_fill_rectangle(0, 0, 319 << 2, 239 << 2),
            gdp_set_cycle_type(0),
            // gsDPSetCombineMode(G_CC_SHADE, G_CC_SHADE)
            (0xFCFF_FFFF, 0xFFFE_793C),
            gdp_set_render_mode(G_RM_OPA_SURF, G_RM_OPA_SURF2),
            gsp_set_geometrymode(0x0000_0004),
            gsp_viewport(0x0600_0000),
            gsp_matrix(0x0600_0080, true, true, false),
            gsp_matrix(0x0600_0040, false, true, false),
            gsp_vertex(0, 3, 0x0600_0100),
            (0xDE00_0000, pointer(SUB_DL)),
            (0xDF00_0000, 0),
        ],
    );
    put_commands(
        &mut bytes,
        SUB_DL,
        &[gsp_1triangle(0, 1, 2), (0xDF00_0000, 0)],
    );
    bytes
}

fn walk(memory: impl Rdram) -> (Vec<String>, String) {
    let mut steps = Steps::default();
    let summary = inspect::walk(
        memory,
        u64::from(MAIN_DL),
        Microcode::F3dzex2,
        DataFormat::Fixed,
        &mut steps,
    );
    (steps.0, format!("{summary:?}"))
}

fn recomp_memory(bytes: &[u8]) -> Vec<AtomicU32> {
    let words: Vec<AtomicU32> = (0..bytes.len() / 4).map(|_| AtomicU32::new(0)).collect();
    GuestMemory::new(&words).write_bytes(RDRAM_BASE, bytes);
    words
}

#[test]
fn same_walk_as_rdram_image() {
    let bytes = scene(|address| address);
    let (expected_steps, expected_summary) = walk(RdramImage::new(&bytes));
    assert!(expected_summary.contains("tris: 1"), "{expected_summary}");
    assert!(
        expected_summary.contains("diagnostics: []"),
        "{expected_summary}"
    );

    let words = recomp_memory(&bytes);
    let (steps, summary) = walk(RecompRdram::new(GuestMemory::new(&words)));
    assert_eq!(summary, expected_summary);
    assert_eq!(steps, expected_steps);
}

#[test]
fn kseg0_pointers_resolve_to_the_same_memory() {
    let (expected_steps, expected_summary) = walk(RdramImage::new(&scene(|address| address)));

    let words = recomp_memory(&scene(|address| 0x8000_0000 | address));
    let (steps, summary) = walk(RecompRdram::new(GuestMemory::new(&words)));
    assert_eq!(summary, expected_summary);
    // The command words themselves differ (they hold the KSEG0 pointers); what they draw doesn't.
    let strip = |steps: &[String]| -> Vec<String> {
        steps
            .iter()
            .map(|s| s.split(", state:").nth(1).unwrap_or(s).to_owned())
            .collect()
    };
    assert_eq!(strip(&steps), strip(&expected_steps));
}
