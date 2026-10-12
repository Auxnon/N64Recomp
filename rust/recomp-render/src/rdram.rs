//! fast3d's view of recomp RDRAM.
//!
//! fast3d reads guest memory through its `Rdram` trait, using physical addresses and
//! big-endian values. Recomp RDRAM is stored as native-endian 32-bit words and addressed
//! from KSEG0 (`rdram[0]` is `0x80000000`), so every read here loads whole words and
//! reassembles the bytes in guest order.
//!
//! Address resolution follows RT64 (`RSP::fromSegmented`, `maskPhysicalAddress` and
//! `RDP::maskAddress`) with its extended-RDRAM mode always on: an address with bit 31 set
//! is a KSEG0 pointer and maps to `address - 0x80000000`, so memory above 8 MB (the mod
//! heap at `0x81000000`) is reachable. Other addresses go through the segment table and
//! are masked to 24 bits as the RSP and RDP do. Zelda64Recomp turns extended mode on with
//! `gEXSetRDRAMExtended(1)` before drawing; keeping it on for every list only changes
//! addresses at or above `0x81000000`, which would otherwise wrap.

use std::borrow::Cow;

use fast3d::{Command, DataFormat, Matrix, MemoryError, MemoryErrorKind, Rdram};
use recomp_abi::{GuestMemory, RDRAM_BASE};

/// Bit 31 marks a KSEG0 pointer in extended-RDRAM mode (RT64's `ExtendedMask`).
const EXTENDED: u32 = 0x8000_0000;
/// The RDP's 24-bit address mask (`RDP_ADDRESS_MASK`).
const RDP_MASK: u32 = 0x00FF_FFFF;
/// The RSP DMA mask: 24 bits, 8-byte aligned.
const RSP_DMA_MASK: u32 = 0x00FF_FFF8;

fn error(address: u64, length: u64, kind: MemoryErrorKind) -> MemoryError {
    MemoryError {
        address,
        length,
        kind,
    }
}

/// Maps a resolved address to an RDRAM offset, applying `mask` unless it is a KSEG0 pointer.
fn physical(address: u32, mask: u32) -> u64 {
    if address & EXTENDED != 0 {
        u64::from(address - EXTENDED)
    } else {
        u64::from(address & mask)
    }
}

/// One display-list walk's reader over guest RDRAM, with its own segment table.
pub struct RecompRdram<'m> {
    mem: GuestMemory<'m>,
    segments: [u32; 16],
}

impl<'m> RecompRdram<'m> {
    pub fn new(mem: GuestMemory<'m>) -> Self {
        Self {
            mem,
            segments: [0; 16],
        }
    }

    fn segmented(&self, address: u64) -> Result<u32, MemoryError> {
        let address = u32::try_from(address)
            .map_err(|_| error(address, 0, MemoryErrorKind::AddressOverflow))?;
        if address & EXTENDED != 0 {
            return Ok(address);
        }
        let base = self.segments[((address >> 24) & 0x0F) as usize];
        Ok(base.wrapping_add(address & 0x00FF_FFFF))
    }

    /// Checks that `length` bytes from `address` are inside RDRAM.
    fn check(&self, address: u64, length: u64) -> Result<(), MemoryError> {
        let end = address
            .checked_add(length)
            .ok_or_else(|| error(address, length, MemoryErrorKind::AddressOverflow))?;
        if end > self.mem.len_bytes() as u64 {
            return Err(error(address, length, MemoryErrorKind::OutOfBounds));
        }
        Ok(())
    }

    /// The word holding RDRAM byte `offset`, in guest byte order. The caller has checked bounds.
    fn word_at(&self, offset: u64) -> [u8; 4] {
        self.mem.read_u32(RDRAM_BASE + (offset & !3)).to_be_bytes()
    }

    fn bytes<const N: usize>(&self, address: u64) -> Result<[u8; N], MemoryError> {
        self.check(address, N as u64)?;
        let mut out = [0; N];
        self.copy(address, &mut out);
        Ok(out)
    }

    /// Copies guest bytes from a checked range, a word at a time.
    fn copy(&self, address: u64, out: &mut [u8]) {
        let mut offset = address;
        let mut filled = 0;
        while filled < out.len() {
            let word = self.word_at(offset);
            let start = (offset & 3) as usize;
            let take = (4 - start).min(out.len() - filled);
            out[filled..filled + take].copy_from_slice(&word[start..start + take]);
            filled += take;
            offset += take as u64;
        }
    }
}

impl Rdram for RecompRdram<'_> {
    fn set_segment(&mut self, segment: u32, value: u64) {
        self.segments[(segment & 0x0F) as usize] = value as u32;
    }

    fn resolve(&self, address: u64) -> Result<u64, MemoryError> {
        Ok(physical(self.segmented(address)?, RDP_MASK))
    }

    fn resolve_masked(&self, address: u64) -> Result<u64, MemoryError> {
        Ok(physical(self.segmented(address)?, RSP_DMA_MASK))
    }

    fn read_command(&self, address: u64) -> Result<Command, MemoryError> {
        let bytes: [u8; 8] = self.bytes(address)?;
        let w0 = u32::from_be_bytes(bytes[..4].try_into().unwrap());
        let w1 = u32::from_be_bytes(bytes[4..].try_into().unwrap());
        Ok(Command {
            w0,
            w1,
            w1_addr: u64::from(w1),
        })
    }

    fn command_stride(&self) -> u64 {
        8
    }

    fn in_bounds(&self, address: u64, length: u64) -> bool {
        self.check(address, length).is_ok()
    }

    fn read_u8(&self, address: u64) -> Result<u8, MemoryError> {
        Ok(self.bytes::<1>(address)?[0])
    }

    fn read_i8(&self, address: u64) -> Result<i8, MemoryError> {
        Ok(self.read_u8(address)? as i8)
    }

    fn read_i16(&self, address: u64) -> Result<i16, MemoryError> {
        Ok(i16::from_be_bytes(self.bytes(address)?))
    }

    fn read_u16(&self, address: u64) -> Result<u16, MemoryError> {
        Ok(u16::from_be_bytes(self.bytes(address)?))
    }

    fn read_bytes(&self, address: u64, length: usize) -> Result<Cow<'_, [u8]>, MemoryError> {
        self.check(address, length as u64)?;
        let mut out = vec![0; length];
        self.copy(address, &mut out);
        Ok(Cow::Owned(out))
    }

    fn read_matrix(&self, address: u64, format: DataFormat) -> Result<Matrix, MemoryError> {
        if format != DataFormat::Fixed {
            return Err(error(address, 64, MemoryErrorKind::UnsupportedFormat));
        }
        let bytes: [u8; 64] = self.bytes(address)?;
        let mut matrix = [[0.0; 4]; 4];
        for (k, cell) in matrix.iter_mut().flatten().enumerate() {
            let integer = i16::from_be_bytes([bytes[k * 2], bytes[k * 2 + 1]]) as i32;
            let fraction = u16::from_be_bytes([bytes[32 + k * 2], bytes[33 + k * 2]]) as i32;
            *cell = ((integer << 16) | fraction) as f32 / 65536.0;
        }
        Ok(matrix)
    }

    /// Resolved addresses are RDRAM offsets, so VI origins can be matched against them.
    fn is_rdram_image(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicU32;

    use super::*;

    fn memory(words: usize) -> Vec<AtomicU32> {
        (0..words).map(|_| AtomicU32::new(0)).collect()
    }

    #[test]
    fn reads_are_in_guest_byte_order() {
        let words = memory(4);
        let mem = GuestMemory::new(&words);
        mem.write_u32(RDRAM_BASE, 0x1234_5678);
        mem.write_u32(RDRAM_BASE + 4, 0x9ABC_DEF0);
        let rdram = RecompRdram::new(mem);
        assert_eq!(rdram.read_u8(0), Ok(0x12));
        assert_eq!(rdram.read_u8(3), Ok(0x78));
        assert_eq!(rdram.read_u16(2), Ok(0x5678));
        assert_eq!(
            rdram.read_u16(3),
            Ok(0x789A),
            "unaligned halfword across a word"
        );
        assert_eq!(rdram.read_i16(4), Ok(0x9ABCu16 as i16));
        assert_eq!(
            rdram.read_bytes(1, 6).unwrap().as_ref(),
            [0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE]
        );
        let command = rdram.read_command(0).unwrap();
        assert_eq!(
            (command.w0, command.w1, command.w1_addr),
            (0x1234_5678, 0x9ABC_DEF0, 0x9ABC_DEF0)
        );
    }

    #[test]
    fn reads_outside_rdram_fail_instead_of_panicking() {
        let words = memory(2);
        let rdram = RecompRdram::new(GuestMemory::new(&words));
        assert_eq!(rdram.read_u8(7), Ok(0));
        assert_eq!(
            rdram.read_u16(7),
            Err(error(7, 2, MemoryErrorKind::OutOfBounds))
        );
        assert_eq!(
            rdram.read_command(4).unwrap_err().kind,
            MemoryErrorKind::OutOfBounds
        );
        assert_eq!(
            rdram.read_bytes(u64::MAX, 2).unwrap_err().kind,
            MemoryErrorKind::AddressOverflow
        );
        assert!(rdram.in_bounds(0, 8));
        assert!(!rdram.in_bounds(1, 8));
    }

    #[test]
    fn segments_and_masks_follow_rt64() {
        let words = memory(1);
        let mut rdram = RecompRdram::new(GuestMemory::new(&words));
        rdram.set_segment(6, 0x0010_0000);
        assert_eq!(rdram.resolve(0x0600_1234), Ok(0x0010_1234));
        assert_eq!(rdram.resolve_masked(0x0600_1234), Ok(0x0010_1230));
        // Physical addresses use segment 0, which is zero.
        assert_eq!(rdram.resolve(0x003E_0000), Ok(0x003E_0000));
        // The RDP and RSP DMA keep 24 bits.
        rdram.set_segment(1, 0x00FF_0000);
        assert_eq!(rdram.resolve(0x0102_0000), Ok(0x0001_0000));
        // KSEG0 pointers bypass the segment table, unmasked, including the mod heap.
        assert_eq!(rdram.resolve(0x8012_3457), Ok(0x0012_3457));
        assert_eq!(rdram.resolve_masked(0x8100_0004), Ok(0x0100_0004));
        // A segment holding a KSEG0 pointer resolves the same way.
        rdram.set_segment(8, 0x8100_0000);
        assert_eq!(rdram.resolve(0x0800_0010), Ok(0x0100_0010));
        assert_eq!(
            rdram.resolve(1 << 32).unwrap_err().kind,
            MemoryErrorKind::AddressOverflow
        );
    }

    #[test]
    fn matrices_decode_integer_and_fraction_halves() {
        let words = memory(16);
        let mem = GuestMemory::new(&words);
        // Element 0 = 1.5, element 5 = -2.25, the rest zero.
        mem.write_u16(RDRAM_BASE, 1);
        mem.write_u16(RDRAM_BASE + 32, 0x8000);
        mem.write_u16(RDRAM_BASE + 10, 0xFFFD);
        mem.write_u16(RDRAM_BASE + 42, 0xC000);
        let matrix = RecompRdram::new(mem)
            .read_matrix(0, DataFormat::Fixed)
            .unwrap();
        assert_eq!(matrix[0][0], 1.5);
        assert_eq!(matrix[1][1], -2.25);
        assert_eq!(matrix[3][3], 0.0);
    }
}
