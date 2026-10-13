//! Saving frames for offline replay (`examples/replay.rs`).
//!
//! With `RECOMP_RENDER_DUMP=<dir>` set, the renderer saves the frames listed in
//! `RECOMP_RENDER_DUMP_FRAMES` (comma-separated presented-frame numbers or `first-last`
//! ranges, default `600`; `none` for no fixed frames). Creating a file named `now` in the
//! directory (`touch <dir>/now`) also saves the next `RECOMP_RENDER_DUMP_COUNT` frames
//! (default 30), for a moment that's hard to time by frame number. For
//! each graphics task in a listed frame it writes a snapshot of RDRAM taken while the task is
//! drawn, and it appends one line per task and one for the VI registers to `frame<N>.txt`:
//!
//! ```text
//! task <k> <ucode> <ucode_data> <ucode_data_size> <data_ptr> <data_size>
//! vi <status> <origin> <width> ... (the 14 ViRegs fields)
//! ```
//!
//! Snapshots are `frame<N>_task<k>.rdram`, big-endian like real RDRAM, from `0x80000000`.

use std::fmt::Write as _;
use std::fs;
use std::io::Write as _;
use std::path::PathBuf;

use recomp_abi::{GuestMemory, RDRAM_BASE};

use crate::context::GfxTask;
use crate::vi::ViRegs;

/// The most RDRAM saved per task: the base 8 MB plus the mod heap above `0x81000000`.
const SNAPSHOT_LIMIT: usize = 64 << 20;
/// The file whose creation starts a triggered dump.
const TRIGGER: &str = "now";

pub struct FrameDump {
    dir: PathBuf,
    frames: Vec<u64>,
    task: u32,
    /// Frames saved per trigger.
    count: u64,
    /// The frames of the last triggered dump.
    triggered: Option<std::ops::RangeInclusive<u64>>,
}

impl FrameDump {
    /// The dump the environment asks for, if any.
    pub fn from_env() -> Option<Self> {
        let dir = PathBuf::from(std::env::var_os("RECOMP_RENDER_DUMP")?);
        let frames = std::env::var("RECOMP_RENDER_DUMP_FRAMES")
            .ok()
            .map(|list| parse_frames(&list))
            .unwrap_or_else(|| vec![600]);
        if let Err(e) = fs::create_dir_all(&dir) {
            eprintln!(
                "[fast3d] can't create dump directory {}: {e}",
                dir.display()
            );
            return None;
        }
        let count = std::env::var("RECOMP_RENDER_DUMP_COUNT")
            .ok()
            .and_then(|count| count.parse().ok())
            .unwrap_or(30u64)
            .min(1000);
        eprintln!(
            "[fast3d] dumping frames {frames:?} to {}; touch {}/{TRIGGER} to dump the next {count}",
            dir.display(),
            dir.display()
        );
        Some(Self {
            dir,
            frames,
            task: 0,
            count,
            triggered: None,
        })
    }

    fn wants(&self, frame: u64) -> bool {
        self.frames.contains(&frame) || self.triggered.as_ref().is_some_and(|r| r.contains(&frame))
    }

    /// Starts a triggered dump after `frame` if the trigger file exists, removing it.
    fn check_trigger(&mut self, frame: u64) {
        let trigger = self.dir.join(TRIGGER);
        if self.triggered.as_ref().is_some_and(|r| frame < *r.end()) || !trigger.exists() {
            return;
        }
        let _ = fs::remove_file(&trigger);
        let frames = frame + 1..=frame + self.count;
        eprintln!("[fast3d] dumping frames {frames:?}");
        self.triggered = Some(frames);
    }

    fn append(&self, frame: u64, line: &str) {
        let path = self.dir.join(format!("frame{frame}.txt"));
        let result = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .and_then(|mut file| writeln!(file, "{line}"));
        if let Err(e) = result {
            eprintln!("[fast3d] can't write {}: {e}", path.display());
        }
    }

    /// Saves `task` and the memory it draws from, if `frame` is listed.
    pub fn task(&mut self, frame: u64, mem: GuestMemory<'_>, task: &GfxTask) {
        if !self.wants(frame) {
            return;
        }
        let k = self.task;
        self.task += 1;
        let path = self.dir.join(format!("frame{frame}_task{k}.rdram"));
        if let Err(e) = fs::write(&path, snapshot(mem)) {
            eprintln!("[fast3d] can't write {}: {e}", path.display());
            return;
        }
        self.append(
            frame,
            &format!(
                "task {k} {:#010x} {:#010x} {:#x} {:#010x} {:#x}",
                task.ucode, task.ucode_data, task.ucode_data_size, task.data_ptr, task.data_size
            ),
        );
    }

    /// Records the VI registers `frame` was presented with.
    pub fn present(&mut self, frame: u64, vi: &ViRegs) {
        if self.wants(frame) {
            self.task = 0;
            let mut line = String::from("vi");
            for value in vi.fields() {
                let _ = write!(line, " {value:#x}");
            }
            self.append(frame, &line);
            eprintln!("[fast3d] dumped frame {frame}");
        }
        // This frame's tasks have been drawn, so a trigger starts with the next frame.
        self.check_trigger(frame);
    }
}

fn parse_frames(list: &str) -> Vec<u64> {
    let mut frames = Vec::new();
    for item in list.split(',').map(str::trim) {
        if let Some((first, last)) = item.split_once('-') {
            if let (Ok(first), Ok(last)) = (first.parse::<u64>(), last.parse::<u64>()) {
                frames.extend(first..=last.min(first + 1000));
            }
        } else if let Ok(frame) = item.parse() {
            frames.push(frame);
        }
    }
    frames
}

/// RDRAM as big-endian bytes, up to [`SNAPSHOT_LIMIT`].
fn snapshot(mem: GuestMemory<'_>) -> Vec<u8> {
    let length = mem.len_bytes().min(SNAPSHOT_LIMIT) & !3;
    let mut out = Vec::with_capacity(length);
    for offset in (0..length as u64).step_by(4) {
        out.extend_from_slice(&mem.read_u32(RDRAM_BASE + offset).to_be_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicU32;

    use super::*;

    #[test]
    fn frame_lists_skip_bad_entries() {
        assert_eq!(parse_frames("1, 20,x,300"), [1, 20, 300]);
        assert_eq!(parse_frames("5-7,9"), [5, 6, 7, 9]);
    }

    #[test]
    fn trigger_dumps_the_frames_after_it() {
        let dir = std::env::temp_dir().join(format!("recomp-dump-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let mut dump = FrameDump {
            dir: dir.clone(),
            frames: Vec::new(),
            task: 0,
            count: 2,
            triggered: None,
        };
        let vi = ViRegs::default();
        dump.present(10, &vi);
        fs::write(dir.join(TRIGGER), "").unwrap();
        dump.present(11, &vi);
        assert!(!dir.join(TRIGGER).exists());
        assert!(!dump.wants(11));
        assert!(dump.wants(12) && dump.wants(13));
        assert!(!dump.wants(14));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn snapshots_are_big_endian() {
        let words: Vec<AtomicU32> = (0..2).map(|_| AtomicU32::new(0)).collect();
        let mem = GuestMemory::new(&words);
        mem.write_u32(RDRAM_BASE, 0x1234_5678);
        assert_eq!(snapshot(mem), [0x12, 0x34, 0x56, 0x78, 0, 0, 0, 0]);
    }
}
