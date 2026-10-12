//! Reporting fast3d's diagnostics and log messages on stderr.
//!
//! A running game repeats the same problems every frame, so each kind of diagnostic is
//! printed the first few times it happens, and a count of every kind is printed
//! periodically and at shutdown. That list is the backlog for Phase 3 of the port.

use std::collections::BTreeMap;
use std::mem::{Discriminant, discriminant};

use fast3d::{DiagKind, DiagSink, Diagnostic};

/// How many of each kind are printed in full.
const PRINT_FIRST: u64 = 3;

struct KindCount {
    name: String,
    count: u64,
    reported: u64,
}

/// Counts diagnostics by kind and prints new ones.
#[derive(Default)]
pub struct DiagLog {
    kinds: Vec<(Discriminant<DiagKind>, KindCount)>,
    verbose: bool,
}

/// The variant name from a diagnostic's `Debug` output.
fn kind_name(kind: &DiagKind) -> String {
    let text = format!("{kind:?}");
    let end = text.find([' ', '{', '(']).unwrap_or(text.len());
    text[..end].to_owned()
}

impl DiagLog {
    pub fn new(verbose: bool) -> Self {
        Self {
            kinds: Vec::new(),
            verbose,
        }
    }

    fn entry(&mut self, kind: &DiagKind) -> &mut KindCount {
        let key = discriminant(kind);
        let index = match self.kinds.iter().position(|(k, _)| *k == key) {
            Some(index) => index,
            None => {
                let count = KindCount {
                    name: kind_name(kind),
                    count: 0,
                    reported: 0,
                };
                self.kinds.push((key, count));
                self.kinds.len() - 1
            }
        };
        &mut self.kinds[index].1
    }

    /// Prints counts of every kind that has changed since the last summary.
    pub fn summarize(&mut self) {
        let mut changed = BTreeMap::new();
        for (_, kind) in &mut self.kinds {
            if kind.count != kind.reported {
                changed.insert(kind.name.clone(), kind.count);
                kind.reported = kind.count;
            }
        }
        if !changed.is_empty() {
            let list: Vec<String> = changed
                .iter()
                .map(|(name, n)| format!("{name} x{n}"))
                .collect();
            eprintln!("[fast3d] diagnostics so far: {}", list.join(", "));
        }
    }
}

impl DiagSink for DiagLog {
    fn emit(&mut self, diag: Diagnostic) {
        let verbose = self.verbose;
        let entry = self.entry(&diag.kind);
        entry.count += 1;
        if verbose || entry.count <= PRINT_FIRST {
            eprintln!(
                "[fast3d] {:?} at {:#x}: {:?}",
                diag.kind.severity(),
                diag.at,
                diag.kind
            );
        }
    }
}

/// Sends fast3d's `log` output to stderr. Level from `RECOMP_RENDER_LOG` (default `warn`).
struct StderrLogger;

impl log::Log for StderrLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::max_level()
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            eprintln!(
                "[{}] {}: {}",
                record.target(),
                record.level(),
                record.args()
            );
        }
    }

    fn flush(&self) {}
}

pub fn install_logger() {
    static LOGGER: StderrLogger = StderrLogger;
    let level = std::env::var("RECOMP_RENDER_LOG")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(log::LevelFilter::Warn);
    // Another logger may already be installed by the host; then it gets the messages.
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(level);
    }
}

#[cfg(test)]
mod tests {
    use fast3d::{MemoryAccess, MemoryError, MemoryErrorKind};

    use super::*;

    #[test]
    fn counts_by_kind() {
        let mut log = DiagLog::new(false);
        let read = DiagKind::MemoryRead {
            access: MemoryAccess::Command,
            error: MemoryError {
                address: 0,
                length: 8,
                kind: MemoryErrorKind::OutOfBounds,
            },
        };
        for at in 0..5 {
            log.emit(Diagnostic { at, kind: read });
        }
        log.emit(Diagnostic {
            at: 0,
            kind: DiagKind::UnsupportedExtendedCommand { command: 0x0A },
        });
        let counts: Vec<_> = log
            .kinds
            .iter()
            .map(|(_, k)| (k.name.as_str(), k.count))
            .collect();
        assert_eq!(
            counts,
            [("MemoryRead", 5), ("UnsupportedExtendedCommand", 1)]
        );
        log.summarize();
        assert!(log.kinds.iter().all(|(_, k)| k.reported == k.count));
    }
}
