//! recomp-runtime's save backend (saving.rs) against librecomp's (`c/saving_reference.cpp`,
//! verbatim from pi.cpp and files.cpp), each in its own scratch directory: the same files to
//! start from, the same operations, then the same in-memory save and the same files on disk.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::time::Duration;

use recomp_abi::{GuestMemory, RDRAM_BASE};
use recomp_runtime::saving::{Host, Saves, save_size};
use recomp_runtime_ctest::harness::Rng;

unsafe extern "C" {
    fn ctest_ref_start(config_dir: *const u8, game: *const u8, save_type: u32);
    fn ctest_ref_write_ptr(input: *const u8, offset: u32, count: u32);
    fn ctest_ref_write(rdram: *mut u8, address: i32, offset: u32, count: u32);
    fn ctest_ref_read(rdram: *mut u8, address: i32, offset: u32, count: u32);
    fn ctest_ref_clear(start: u32, size: u32, value: u8);
    fn ctest_ref_change(subfolder: *const u8, name: *const u8);
    fn ctest_ref_path(out: *mut u8, capacity: usize) -> usize;
    fn ctest_ref_snapshot(out: *mut u8, capacity: usize) -> usize;
    fn ctest_ref_reported() -> i32;
    fn ctest_ref_exit();
}

const FLASHRAM: u32 = 4;
const GAME: &str = "game";
/// Long enough for both saving threads to finish a burst (they wait 10 ms for more writes).
const SETTLE: Duration = Duration::from_millis(120);

static RUST_EXITED: AtomicBool = AtomicBool::new(false);
static RUST_REPORTED: AtomicUsize = AtomicUsize::new(0);

fn c(text: &str) -> Vec<u8> {
    let mut bytes = text.as_bytes().to_vec();
    bytes.push(0);
    bytes
}

fn reference_snapshot() -> Vec<u8> {
    let mut out = vec![0; 0x20000];
    let n = unsafe { ctest_ref_snapshot(out.as_mut_ptr(), out.len()) };
    out.truncate(n);
    out
}

fn reference_path() -> PathBuf {
    let mut out = vec![0; 4096];
    let n = unsafe { ctest_ref_path(out.as_mut_ptr(), out.len()) };
    out.truncate(n);
    PathBuf::from(String::from_utf8(out).unwrap())
}

/// Every file under `dir`, relative path and contents, sorted.
fn files(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in fs::read_dir(&d).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else {
                out.push((
                    path.strip_prefix(dir).unwrap().to_path_buf(),
                    fs::read(&path).unwrap(),
                ));
            }
        }
    }
    out.sort();
    out
}

/// The same starting files in both save folders.
fn seed(folders: [&Path; 2], rng: &mut Rng) {
    let random =
        |rng: &mut Rng, n: usize| (0..n).map(|_| rng.next_u64() as u8).collect::<Vec<u8>>();
    let full = random(rng, 0x20000);
    let backup_only = random(rng, 0x20000);
    let short = random(rng, 300);
    for folder in folders {
        fs::create_dir_all(folder.join("modA")).unwrap();
        fs::create_dir_all(folder.join("modB")).unwrap();
        fs::write(folder.join(format!("{GAME}.bin")), &full).unwrap();
        fs::write(folder.join("modA/slot.bin.bak"), &backup_only).unwrap();
        fs::write(folder.join("modB/short.bin"), &short).unwrap();
    }
}

struct Sides {
    rust: &'static Saves,
    reference_mem: Vec<AtomicU32>,
    rust_mem: Vec<AtomicU32>,
    folders: [PathBuf; 2],
}

impl Sides {
    /// One burst of random operations on both sides; fast enough to be one save each.
    fn burst(&self, rng: &mut Rng) {
        let rust_mem = GuestMemory::new(&self.rust_mem);
        let reference_rdram = self.reference_mem.as_ptr().cast_mut().cast::<u8>();
        for _ in 0..1 + rng.below(40) {
            let count = 1 + rng.below(512) as u32;
            let offset = rng.below(0x20000 - u64::from(count)) as u32;
            let address = (RDRAM_BASE + rng.below(0x1000 - u64::from(count.min(0x1000)))) as i32;
            match rng.below(4) {
                0 => {
                    let bytes: Vec<u8> = (0..count).map(|_| rng.next_u64() as u8).collect();
                    unsafe { ctest_ref_write_ptr(bytes.as_ptr(), offset, count) };
                    self.rust.write_bytes(&bytes, offset);
                }
                1 => {
                    let value = rng.next_u64() as u8;
                    unsafe { ctest_ref_clear(offset, count, value) };
                    self.rust.clear(offset, count, value);
                }
                2 => {
                    let count = count.min(0x800);
                    unsafe { ctest_ref_write(reference_rdram, address, offset, count) };
                    self.rust.write(&rust_mem, address, offset, count);
                }
                _ => {
                    let count = count.min(0x800);
                    unsafe { ctest_ref_read(reference_rdram, address, offset, count) };
                    self.rust.read(&rust_mem, address, offset, count);
                }
            }
        }
    }

    fn assert_same(&self, when: &str) {
        assert!(
            self.rust.snapshot() == reference_snapshot(),
            "{when}: in-memory save"
        );
        let [reference, rust] = [0, 1].map(|i| files(&self.folders[i]));
        let names: Vec<_> = reference.iter().map(|(p, _)| p.clone()).collect();
        assert_eq!(
            names,
            rust.iter().map(|(p, _)| p.clone()).collect::<Vec<_>>(),
            "{when}: files"
        );
        assert!(reference == rust, "{when}: file contents");
        assert!(
            names.iter().all(|p| p.extension() != Some("temp".as_ref())),
            "{when}: temp file left"
        );
        let memory = |m: &[AtomicU32]| {
            m.iter()
                .map(|w| w.load(Ordering::Relaxed))
                .collect::<Vec<_>>()
        };
        assert!(
            memory(&self.reference_mem) == memory(&self.rust_mem),
            "{when}: guest memory"
        );
    }
}

#[test]
fn save_backend_matches_the_cpp_on_disk() {
    let root = std::env::temp_dir().join(format!("recomp-saving-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    // The C++ adds "saves" to its config path; Rust takes the save folder itself.
    let folders = [root.join("reference/saves"), root.join("rust/saves")];
    let mut rng = Rng(0xB4C_0FF5);
    seed([&folders[0], &folders[1]], &mut rng);

    unsafe {
        ctest_ref_start(
            c(root.join("reference").to_str().unwrap()).as_ptr(),
            c(GAME).as_ptr(),
            FLASHRAM,
        )
    };
    let rust = Saves::start(
        folders[1].clone(),
        GAME,
        save_size(FLASHRAM),
        Host {
            exited: || RUST_EXITED.load(Ordering::Relaxed),
            report: |_| {
                RUST_REPORTED.fetch_add(1, Ordering::Relaxed);
            },
        },
    );
    let memory = || {
        (0..0x1000 / 4)
            .map(|i| AtomicU32::new(i * 0x0101_0101))
            .collect::<Vec<_>>()
    };
    let sides = Sides {
        rust,
        reference_mem: memory(),
        rust_mem: memory(),
        folders,
    };
    assert_eq!(reference_path().file_name(), rust.path().file_name());
    sides.assert_same("after reading an existing file");

    for round in 0..12 {
        sides.burst(&mut rng);
        std::thread::sleep(SETTLE);
        sides.assert_same(&format!("after burst {round}"));
        match round {
            3 => {
                // Only a backup exists: it's read instead.
                unsafe { ctest_ref_change(c("modA").as_ptr(), c("slot").as_ptr()) };
                rust.change_file("modA", "slot");
                assert_eq!(reference_path().file_name(), rust.path().file_name());
                sides.assert_same("after switching to a file with only a backup");
            }
            6 => {
                // A short file fills the start; the rest keeps what was there.
                unsafe { ctest_ref_change(c("modB").as_ptr(), c("short").as_ptr()) };
                rust.change_file("modB", "short");
                sides.assert_same("after switching to a short file");
            }
            8 => {
                // A save that can't be written is reported and leaves the file as it was.
                for folder in &sides.folders {
                    fs::create_dir(folder.join("modB/short.bin.temp")).unwrap();
                }
                let before = files(&sides.folders[1]);
                sides.burst(&mut rng);
                std::thread::sleep(SETTLE);
                assert_eq!(
                    unsafe { ctest_ref_reported() },
                    1,
                    "C++ reported the failure"
                );
                assert_eq!(
                    RUST_REPORTED.load(Ordering::Relaxed),
                    1,
                    "Rust reported the failure"
                );
                assert!(
                    files(&sides.folders[1]) == before,
                    "failed save left the files alone"
                );
                for folder in &sides.folders {
                    fs::remove_dir(folder.join("modB/short.bin.temp")).unwrap();
                }
            }
            _ => {}
        }
    }

    unsafe { ctest_ref_exit() };
    RUST_EXITED.store(true, Ordering::Relaxed);
    rust.join();
    sides.assert_same("after exiting");
    let _ = fs::remove_dir_all(&root);
}
