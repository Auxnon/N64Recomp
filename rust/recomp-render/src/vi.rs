//! VI registers from ultramodern, in the form fast3d scans out from.

use fast3d::ViRegisters;

/// `ultramodern::renderer::ViRegs`, field for field.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ViRegs {
    pub status: u32,
    pub origin: u32,
    pub width: u32,
    pub intr: u32,
    pub v_current_line: u32,
    pub timing: u32,
    pub v_sync: u32,
    pub h_sync: u32,
    pub leap: u32,
    pub h_start: u32,
    pub v_start: u32,
    pub v_burst: u32,
    pub x_scale: u32,
    pub y_scale: u32,
}

impl ViRegs {
    /// The registers fast3d reads.
    ///
    /// ultramodern sets `VI_ORIGIN` to the framebuffer plus the mode's field origin, which
    /// for the common non-interlaced modes is one line (`osViModeNtscLan1`: 640 bytes at
    /// 320 pixels, 16 bpp). fast3d looks framebuffers up by their exact start address, so
    /// the line is taken back off. If the result isn't a framebuffer fast3d drew, it shows
    /// the last one drawn instead.
    pub fn to_fast3d(self) -> ViRegisters {
        let bytes_per_pixel = match self.status & 3 {
            2 => 2,
            3 => 4,
            _ => 0,
        };
        let line = (self.width & 0xFFF) * bytes_per_pixel;
        let origin = self.origin & 0x00FF_FFFF;
        ViRegisters {
            status: self.status,
            origin: origin.checked_sub(line).unwrap_or(origin),
            width: self.width,
            x_scale: self.x_scale,
            y_scale: self.y_scale,
            h_start: self.h_start,
            v_start: self.v_start,
            v_current: self.v_current_line,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_drops_the_field_offset_line() {
        let regs = ViRegs {
            status: 0x3216, // 16 bpp
            origin: 0x003E_0280,
            width: 320,
            v_start: (37 << 16) | 511,
            y_scale: 1024,
            ..Default::default()
        };
        let vi = regs.to_fast3d();
        assert_eq!(vi.origin, 0x003E_0000);
        assert_eq!(
            (vi.width, vi.v_start, vi.y_scale),
            (320, regs.v_start, 1024)
        );

        let rgba32 = ViRegs {
            status: 0x3203,
            origin: 0x0010_0500,
            width: 320,
            ..regs
        };
        assert_eq!(rgba32.to_fast3d().origin, 0x0010_0000);

        // Blank (no pixel type) and underflowing origins are left alone.
        assert_eq!(ViRegs { status: 0, ..regs }.to_fast3d().origin, 0x003E_0280);
        assert_eq!(
            ViRegs {
                origin: 0x100,
                ..regs
            }
            .to_fast3d()
            .origin,
            0x100
        );
    }
}
