//! bitfield helpers.

/// extract bits hi..=lo (inclusive) from value.
#[inline(always)]
pub const fn bits(value: u32, lo: u32, hi: u32) -> u32 {
    (value >> lo) & ((1u32 << (hi - lo + 1)).wrapping_sub(1))
}

/// extract a single bit as a bool.
#[inline(always)]
pub const fn bit(value: u32, n: u32) -> bool {
    (value >> n) & 1 != 0
}

/// sign-extend the low n bits of value to a full i32.
#[inline(always)]
pub const fn sign_extend(value: u32, n: u32) -> i32 {
    let shift = 32 - n;
    ((value << shift) as i32) >> shift
}

/// round value up to the next multiple of align (a power of two).
#[inline(always)]
pub const fn align_up(value: u32, align: u32) -> u32 {
    (value + align - 1) & !(align - 1)
}
