use std::mem::{align_of, offset_of, size_of};

use recomp_abi::{Fpr, RecompContext};
use recomp_abi_ctest::ctest_layout;

#[test]
fn recomp_context_layout_matches_recomp_h() {
    let mut c = [0usize; 13];
    unsafe { ctest_layout(&mut c) };

    let rust = [
        size_of::<RecompContext>(),
        align_of::<RecompContext>(),
        offset_of!(RecompContext, r),
        offset_of!(RecompContext, r) + 31 * size_of::<u64>(),
        offset_of!(RecompContext, f),
        offset_of!(RecompContext, f) + 31 * size_of::<Fpr>(),
        offset_of!(RecompContext, hi),
        offset_of!(RecompContext, lo),
        offset_of!(RecompContext, f_odd),
        offset_of!(RecompContext, status_reg),
        offset_of!(RecompContext, mips3_float_mode),
        size_of::<Fpr>(),
        4, // fh/u32h: the high half of the register
    ];
    let names = [
        "sizeof(recomp_context)", "alignof(recomp_context)", "r0", "r31", "f0", "f31", "hi",
        "lo", "f_odd", "status_reg", "mips3_float_mode", "sizeof(fpr)", "offsetof(fpr, fh)",
    ];
    for i in 0..c.len() {
        assert_eq!(rust[i], c[i], "{} differs: rust {} vs C {}", names[i], rust[i], c[i]);
    }
}

#[test]
fn fpr_views_match_the_c_union() {
    let mut f = Fpr::default();
    f.set_fl(1.5);
    f.set_fh(-2.0);
    assert_eq!(f.u32l(), 1.5f32.to_bits());
    assert_eq!(f.u32h(), (-2.0f32).to_bits());
    assert_eq!(f.0, (u64::from((-2.0f32).to_bits()) << 32) | u64::from(1.5f32.to_bits()));
    f.set_d(3.25);
    assert_eq!(f.d(), 3.25);
    assert_eq!(f.0, 3.25f64.to_bits());
}
