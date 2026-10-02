//! SHA-256, for the one download the gate verifies before any tool that
//! could verify it is installed: mise itself, fetched on every platform the
//! toolbelt is pinned for, where `sha256sum` exists on Linux alone.

use std::fmt::Write as _;

/// The first 32 bits of the fractional parts of the cube roots of the first
/// 64 primes, one per round.
const ROUNDS: [u32; 64] = [
    0x428a_2f98,
    0x7137_4491,
    0xb5c0_fbcf,
    0xe9b5_dba5,
    0x3956_c25b,
    0x59f1_11f1,
    0x923f_82a4,
    0xab1c_5ed5,
    0xd807_aa98,
    0x1283_5b01,
    0x2431_85be,
    0x550c_7dc3,
    0x72be_5d74,
    0x80de_b1fe,
    0x9bdc_06a7,
    0xc19b_f174,
    0xe49b_69c1,
    0xefbe_4786,
    0x0fc1_9dc6,
    0x240c_a1cc,
    0x2de9_2c6f,
    0x4a74_84aa,
    0x5cb0_a9dc,
    0x76f9_88da,
    0x983e_5152,
    0xa831_c66d,
    0xb003_27c8,
    0xbf59_7fc7,
    0xc6e0_0bf3,
    0xd5a7_9147,
    0x06ca_6351,
    0x1429_2967,
    0x27b7_0a85,
    0x2e1b_2138,
    0x4d2c_6dfc,
    0x5338_0d13,
    0x650a_7354,
    0x766a_0abb,
    0x81c2_c92e,
    0x9272_2c85,
    0xa2bf_e8a1,
    0xa81a_664b,
    0xc24b_8b70,
    0xc76c_51a3,
    0xd192_e819,
    0xd699_0624,
    0xf40e_3585,
    0x106a_a070,
    0x19a4_c116,
    0x1e37_6c08,
    0x2748_774c,
    0x34b0_bcb5,
    0x391c_0cb3,
    0x4ed8_aa4a,
    0x5b9c_ca4f,
    0x682e_6ff3,
    0x748f_82ee,
    0x78a5_636f,
    0x84c8_7814,
    0x8cc7_0208,
    0x90be_fffa,
    0xa450_6ceb,
    0xbef9_a3f7,
    0xc671_78f2,
];

/// The first 32 bits of the fractional parts of the square roots of the
/// first 8 primes: the state before the first block.
const INITIAL: [u32; 8] = [
    0x6a09_e667,
    0xbb67_ae85,
    0x3c6e_f372,
    0xa54f_f53a,
    0x510e_527f,
    0x9b05_688c,
    0x1f83_d9ab,
    0x5be0_cd19,
];

/// The SHA-256 of `bytes`, as 64 lowercase hex digits.
pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    let bits = u64::try_from(bytes.len())
        .unwrap_or(u64::MAX)
        .wrapping_mul(8);
    let mut message = bytes.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bits.to_be_bytes());
    let mut state = INITIAL;
    for block in message.as_chunks::<64>().0 {
        compress(&mut state, block);
    }
    state.iter().fold(String::new(), |mut hex, word| {
        let _ = write!(hex, "{word:08x}");
        hex
    })
}

/// Fold one 64-byte `block` into `state`.
fn compress(state: &mut [u32; 8], block: &[u8]) {
    let mut schedule: Vec<u32> = block
        .as_chunks::<4>()
        .0
        .iter()
        .map(|chunk| u32::from_be_bytes(*chunk))
        .collect();
    for _ in schedule.len()..ROUNDS.len() {
        let back = |distance: usize| {
            schedule
                .get(schedule.len() - distance)
                .copied()
                .unwrap_or_default()
        };
        let (early, late) = (back(15), back(2));
        let small0 = early.rotate_right(7) ^ early.rotate_right(18) ^ (early >> 3);
        let small1 = late.rotate_right(17) ^ late.rotate_right(19) ^ (late >> 10);
        let word = back(16)
            .wrapping_add(small0)
            .wrapping_add(back(7))
            .wrapping_add(small1);
        schedule.push(word);
    }
    let mut work = *state;
    for (round, word) in ROUNDS.iter().zip(schedule) {
        let [wa, wb, wc, wd, we, wf, wg, wh] = work;
        let big1 = we.rotate_right(6) ^ we.rotate_right(11) ^ we.rotate_right(25);
        let choice = wg ^ (we & (wf ^ wg));
        let first = wh
            .wrapping_add(big1)
            .wrapping_add(choice)
            .wrapping_add(*round)
            .wrapping_add(word);
        let big0 = wa.rotate_right(2) ^ wa.rotate_right(13) ^ wa.rotate_right(22);
        let majority = (wa | wb) & (wc | (wa & wb));
        let second = big0.wrapping_add(majority);
        work = [
            first.wrapping_add(second),
            wa,
            wb,
            wc,
            wd.wrapping_add(first),
            we,
            wf,
            wg,
        ];
    }
    for (value, worked) in state.iter_mut().zip(work) {
        *value = value.wrapping_add(worked);
    }
}

#[cfg(test)]
mod tests {
    use super::sha256_hex;

    #[test]
    fn the_digest_matches_the_published_test_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        // Two blocks: the padding no longer fits in the first.
        assert_eq!(
            sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        assert_eq!(
            sha256_hex(&[b'a'; 1000]),
            "41edece42d63e8d9bf515a9ba6932e1c20cbc9f5a5d134645adb5db1b9737ea3"
        );
    }
}
