pub(super) fn sample(seed: u64, stream: u64, index: usize) -> u64 {
    splitmix64(seed ^ stream ^ (index as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15))
}

pub(super) fn range_i32(seed: u64, stream: u64, index: usize, min: i32, max: i32) -> i32 {
    debug_assert!(min <= max);
    let width = (max - min + 1) as u64;
    min + (sample(seed, stream, index) % width) as i32
}

pub(super) fn shuffle<T: Copy, const N: usize>(seed: u64, stream: u64, values: &mut [T; N]) {
    for upper in (1..N).rev() {
        let selected = (sample(seed, stream, upper) % (upper as u64 + 1)) as usize;
        values.swap(upper, selected);
    }
}

const fn splitmix64(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}
