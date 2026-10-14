//! The save file behind EEPROM, SRAM and FlashRAM (librecomp's save buffer in `pi.cpp`, and
//! the `.temp`/`.bak` handling from `files.cpp`).
//!
//! The game reads and writes an in-memory copy. Each write wakes a saving thread, which waits
//! for writes to stop for 10 ms (or 128 of them) and then writes the whole copy out: first to
//! `<file>.temp`, then the current file is copied to `<file>.bak`, the temp file copied over
//! the file and removed. A file that's missing or can't be opened is read from its `.bak`.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex, MutexGuard, OnceLock};
use std::thread::JoinHandle;
use std::time::Duration;

use recomp_abi::GuestMemory;

/// How long the saving thread waits for another write before saving.
const COALESCE_WAIT: Duration = Duration::from_micros(10_000);
/// The most writes that coalesce into one save.
const MAX_COALESCED: u32 = 128;

/// A counting semaphore (moodycamel's `LightweightSemaphore`, as the C++ uses it).
#[derive(Default)]
struct Semaphore {
    count: Mutex<u64>,
    available: Condvar,
}

impl Semaphore {
    fn signal(&self) {
        *lock(&self.count) += 1;
        self.available.notify_one();
    }

    /// Takes one count, waiting up to `timeout`. Returns whether it got one.
    fn wait_timeout(&self, timeout: Duration) -> bool {
        let count = lock(&self.count);
        let (mut count, _) = self
            .available
            .wait_timeout_while(count, timeout, |count| *count == 0)
            .unwrap_or_else(|e| e.into_inner());
        if *count == 0 {
            return false;
        }
        *count -= 1;
        true
    }

    fn wait(&self) {
        let count = lock(&self.count);
        let mut count = self
            .available
            .wait_while(count, |count| *count == 0)
            .unwrap_or_else(|e| e.into_inner());
        *count -= 1;
    }

    fn try_wait(&self) -> bool {
        let mut count = lock(&self.count);
        if *count == 0 {
            return false;
        }
        *count -= 1;
        true
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// What the saving thread asks of its host: whether the game is exiting, and how to report a
/// failed save.
pub struct Host {
    pub exited: fn() -> bool,
    pub report: fn(&str),
}

/// One save file and its in-memory copy.
pub struct Saves {
    /// `<config>/saves`.
    folder: PathBuf,
    path: Mutex<PathBuf>,
    buffer: Mutex<Vec<u8>>,
    writes: Semaphore,
    swap_pending: Semaphore,
    swap_ready: Semaphore,
    host: Host,
    thread: Mutex<Option<JoinHandle<()>>>,
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// The size of each save type (librecomp's `get_save_size`, by `recomp::SaveType` index).
pub fn save_size(save_type: u32) -> usize {
    match save_type {
        5 | 4 => 0x20000, // AllowAll, Flashram
        3 => 0x8000,      // Sram
        2 => 0x800,       // Eep16k
        1 => 0x200,       // Eep4k
        _ => 0,
    }
}

impl Saves {
    /// The save file `<folder>/<game id>.bin`, read into a `size`-byte copy, with its saving
    /// thread running (`ultramodern::init_saving`).
    pub fn start(folder: PathBuf, game_id: &str, size: usize, host: Host) -> &'static Self {
        let saves: &'static Self = Box::leak(Box::new(Self {
            path: Mutex::new(Self::file_path(&folder, "", game_id)),
            folder,
            buffer: Mutex::new(vec![0; size]),
            writes: Semaphore::default(),
            swap_pending: Semaphore::default(),
            swap_ready: Semaphore::default(),
            host,
            thread: Mutex::new(None),
        }));
        saves.read_file();
        *lock(&saves.thread) = Some(std::thread::spawn(move || saves.saving_thread()));
        saves
    }

    fn file_path(folder: &Path, subfolder: &str, name: &str) -> PathBuf {
        let mut path = folder.to_path_buf();
        if !subfolder.is_empty() {
            path.push(subfolder);
        }
        path.push(format!("{name}.bin"));
        path
    }

    pub fn path(&self) -> PathBuf {
        lock(&self.path).clone()
    }

    /// Fills the copy from the file, or its `.bak`; zeroes it if neither opens. A short file
    /// leaves the rest of the copy as it was.
    fn read_file(&self) {
        let path = self.path();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let file = fs::File::open(&path).or_else(|_| fs::File::open(with_suffix(&path, ".bak")));
        let mut buffer = lock(&self.buffer);
        match file {
            Ok(mut file) => {
                let mut filled = 0;
                while filled < buffer.len() {
                    match file.read(&mut buffer[filled..]) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => filled += n,
                    }
                }
            }
            Err(_) => buffer.fill(0),
        }
    }

    /// Writes the copy to `<file>.temp`, keeps the current file as `<file>.bak`, and moves the
    /// temp file into place.
    fn write_file(&self) {
        let path = self.path();
        let temp = with_suffix(&path, ".temp");
        let written = fs::File::create(&temp).is_ok_and(|mut file| {
            let buffer = lock(&self.buffer);
            // A failed write still counts as written, as in C++ (it doesn't check the stream).
            let _ = file.write_all(&buffer);
            true
        });
        if !written || !Self::finalize(&path, &temp) {
            (self.host.report)(
                "Failed to write to the save file. Check your file permissions and whether the save folder has been moved to Dropbox or similar, as this can cause issues.",
            );
        }
    }

    /// `recomp::finalize_output_file_with_backup`.
    fn finalize(path: &Path, temp: &Path) -> bool {
        if path.exists() && fs::copy(path, with_suffix(path, ".bak")).is_err() {
            return false;
        }
        if fs::copy(temp, path).is_err() {
            return false;
        }
        let _ = fs::remove_file(temp);
        true
    }

    fn saving_thread(&self) {
        while !(self.host.exited)() {
            let mut updated = false;
            let mut actions = 0;
            // Let writes coalesce, but save at least every 128 of them.
            while self.writes.wait_timeout(COALESCE_WAIT) && actions < MAX_COALESCED {
                updated = true;
                actions += 1;
            }
            if updated {
                self.write_file();
            }
            if self.swap_pending.try_wait() {
                self.swap_ready.signal();
            }
        }
    }

    /// Switches to `<folder>/<subfolder>/<name>.bin` once pending writes are saved
    /// (`ultramodern::change_save_file`).
    pub fn change_file(&self, subfolder: &str, name: &str) {
        self.swap_pending.signal();
        self.swap_ready.wait();
        *lock(&self.path) = Self::file_path(&self.folder, subfolder, name);
        self.read_file();
    }

    /// Waits for the saving thread to finish, once the host reports exiting.
    pub fn join(&self) {
        if let Some(thread) = lock(&self.thread).take() {
            let _ = thread.join();
        }
    }

    /// `count` bytes from guest memory at `address` into the copy at `offset`.
    pub fn write(&self, mem: &GuestMemory, address: i32, offset: u32, count: u32) {
        {
            let mut buffer = lock(&self.buffer);
            let address = i64::from(address) as u64;
            for i in 0..count {
                buffer[(offset + i) as usize] = mem.read_u8(address.wrapping_add(u64::from(i)));
            }
        }
        self.writes.signal();
    }

    /// `bytes` into the copy at `offset`.
    pub fn write_bytes(&self, bytes: &[u8], offset: u32) {
        {
            let mut buffer = lock(&self.buffer);
            let offset = offset as usize;
            buffer[offset..offset + bytes.len()].copy_from_slice(bytes);
        }
        self.writes.signal();
    }

    /// `count` bytes from the copy at `offset` into guest memory at `address`.
    pub fn read(&self, mem: &GuestMemory, address: i32, offset: u32, count: u32) {
        let buffer = lock(&self.buffer);
        let address = i64::from(address) as u64;
        for i in 0..count {
            mem.write_u8(address.wrapping_add(u64::from(i)), buffer[(offset + i) as usize]);
        }
    }

    /// A copy of the in-memory save.
    pub fn snapshot(&self) -> Vec<u8> {
        lock(&self.buffer).clone()
    }

    /// Fills `size` bytes of the copy from `start` with `value`.
    pub fn clear(&self, start: u32, size: u32, value: u8) {
        {
            let mut buffer = lock(&self.buffer);
            let start = start as usize;
            buffer[start..start + size as usize].fill(value);
        }
        self.writes.signal();
    }
}

static SAVES: OnceLock<&'static Saves> = OnceLock::new();

/// The game's save file, once [`init`] has run.
pub fn saves() -> &'static Saves {
    SAVES.get().expect("saving used before ultramodern::init_saving")
}

/// The game's save file, if [`init`] has run.
pub fn try_saves() -> Option<&'static Saves> {
    SAVES.get().copied()
}

/// Starts the game's save file (from `ultramodern::init_saving`).
pub fn init(folder: PathBuf, game_id: &str, size: usize, host: Host) {
    let _ = SAVES.set(Saves::start(folder, game_id, size, host));
}
