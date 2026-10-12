//! Prints the RDP-side outline of one dumped task (see `src/dump.rs`): image switches, fills,
//! rectangles, the colors they use, and how many triangles land between them.
//!
//! ```text
//! cargo run -p recomp-render --example inspect -- <dump dir> <frame> [task]
//! ```
//!
//! Set `ALL=1` to print every command. `fast3d::inspect::walk` stops after 4,096 commands,
//! which is short of a full Majora's Mask frame.

use std::ops::ControlFlow;
use std::path::PathBuf;
use std::sync::atomic::AtomicU32;

use fast3d::inspect::{WalkObserver, WalkStep, walk};
use fast3d::{DataFormat, Microcode};
use recomp_abi::GuestMemory;
use recomp_render::rdram::RecompRdram;

struct Outline {
    tris: u32,
    last_tris: u32,
}

impl Outline {
    fn flush(&mut self) {
        if self.tris != self.last_tris {
            println!("        ... {} triangles", self.tris - self.last_tris);
            self.last_tris = self.tris;
        }
    }
}

impl WalkObserver for Outline {
    fn command(&mut self, step: WalkStep<'_>) -> ControlFlow<()> {
        let Some(word) = step.words.first() else {
            return ControlFlow::Continue(());
        };
        let (w0, w1) = (word.w0, word.w1);
        let name = match w0 >> 24 {
            0xFF => Some("SETCIMG"),
            0xFE => Some("SETZIMG"),
            0xFD => Some("SETTIMG"),
            0xF6 => Some("FILLRECT"),
            0xF7 => Some("SETFILLCOLOR"),
            0xFB => Some("SETENVCOLOR"),
            0xFA => Some("SETPRIMCOLOR"),
            0xF9 => Some("SETBLENDCOLOR"),
            0xF8 => Some("SETFOGCOLOR"),
            0xFC => Some("SETCOMBINE"),
            0xE4 => Some("TEXRECT"),
            0xE5 => Some("TEXRECTFLIP"),
            0xE2 => Some("SETOTHERMODE_L"),
            0xE3 => Some("SETOTHERMODE_H"),
            0xEF => Some("RDPSETOTHERMODE"),
            0xDB if (w0 >> 16) & 0xFF == 6 => Some("MOVEWORD SEGMENT"),
            _ => None,
        };
        let interesting = matches!(w0 >> 24, 0xFF | 0xFE | 0xF6 | 0xE4 | 0xE5 | 0xF7)
            || !step.diagnostics.is_empty();
        let tris = step
            .emissions
            .iter()
            .filter(|e| format!("{e:?}").starts_with("Triangle"))
            .count() as u32;
        self.tris += tris;
        if interesting || std::env::var_os("ALL").is_some() {
            self.flush();
            println!(
                "{:#010x} {:08x} {:08x} {}",
                step.pc,
                w0,
                w1,
                name.unwrap_or("")
            );
            for e in step.emissions {
                let text = format!("{e:?}");
                if !text.starts_with("Triangle") {
                    println!("        emits {}", &text[..text.len().min(300)]);
                }
            }
            for d in step.diagnostics {
                println!("        DIAG {d:?}");
            }
        }
        ControlFlow::Continue(())
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = PathBuf::from(args.next().expect("usage: inspect <dir> <frame> [task]"));
    let frame: u64 = args.next().unwrap().parse().unwrap();
    let task: u32 = args.next().map_or(0, |t| t.parse().unwrap());
    let text = std::fs::read_to_string(dir.join(format!("frame{frame}.txt"))).unwrap();
    let line = text
        .lines()
        .find(|l| l.starts_with(&format!("task {task} ")))
        .expect("no such task");
    let data_ptr = u32::from_str_radix(
        line.split_whitespace()
            .nth(5)
            .unwrap()
            .trim_start_matches("0x"),
        16,
    )
    .unwrap();
    let bytes = std::fs::read(dir.join(format!("frame{frame}_task{task}.rdram"))).unwrap();
    let words: Vec<AtomicU32> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|w| AtomicU32::new(u32::from_be_bytes(*w)))
        .collect();
    let rdram = RecompRdram::new(GuestMemory::new(&words));
    let mut outline = Outline {
        tris: 0,
        last_tris: 0,
    };
    let summary = walk(
        rdram,
        u64::from(data_ptr & 0x1FFF_FFFF),
        Microcode::F3dzex2,
        DataFormat::Fixed,
        &mut outline,
    );
    outline.flush();
    println!(
        "dispatched {} tris {} dropped {} termination {:?}",
        summary.dispatched, summary.tris, summary.dropped_runs, summary.termination
    );
}
