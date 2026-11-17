//! Fuzzes the Type 1 to Type1C (CFF) converter (`ariad_pdf::fonts::cff::type1_to_cff`).
//!
//! The first input byte selects what the rest means, so mutation reaches every parser layer:
//! - 0: the whole program, passed through untouched (header and eexec detection).
//! - 1: the decrypted private part, wrapped in a valid cleartext header and eexec-encrypted
//!   (Subrs and CharStrings parsing).
//! - 2: one glyph charstring inside an otherwise valid font with the standard flex and hint
//!   replacement subroutines (the Type 1 interpreter and the Type 2 encoder).
//!
//! Invariants: the converter neither panics nor allocates past its limit, and any accepted
//! program yields a CFF with the 1.0 header and a size within the limit.
#![no_main]

use ariad_pdf::fonts::cff::type1_to_cff;
use libfuzzer_sys::fuzz_target;

const MAX_FUZZ_INPUT: usize = 128 * 1024;
const MAX_OUTPUT: usize = 1 << 20;

const HEAD: &[u8] = b"%!PS-AdobeFont-1.0: Fuzz 1.0\n11 dict begin\n/FontName /Fuzz def\n/PaintType 0 def\n/FontType 1 def\n/FontMatrix [0.001 0 0 0.001 0 0] readonly def\n/Encoding StandardEncoding def\n/FontBBox {-10 -20 900 800} readonly def\ncurrentdict end\ncurrentfile eexec\n";
const PRIVATE_PREFIX: &[u8] = b"dup /Private 8 dict dup begin\n/RD {string currentfile exch readstring pop} executeonly def\n/ND {noaccess def} executeonly def\n/NP {noaccess put} executeonly def\n/BlueValues [-15 0 700 715] def\n";

fn crypt(input: &[u8], key: u16, encrypt: bool) -> Vec<u8> {
    let mut state = key;
    input
        .iter()
        .map(|&byte| {
            let value = byte ^ (state >> 8) as u8;
            let cipher = if encrypt { value } else { byte };
            state = (u32::from(cipher)
                .wrapping_add(u32::from(state))
                .wrapping_mul(52845)
                .wrapping_add(22719)
                & 65535) as u16;
            value
        })
        .collect()
}

fn seal_charstring(plain: &[u8]) -> Vec<u8> {
    let mut data = vec![0x5a, 0x11, 0x22, 0x33];
    data.extend_from_slice(plain);
    crypt(&data, 4330, true)
}

fn wrap_private(private: &[u8]) -> Vec<u8> {
    let mut plain = vec![0x77, 0x31, 0x42, 0x53];
    plain.extend_from_slice(private);
    let encrypted = crypt(&plain, 55665, true);
    let mut program = HEAD.to_vec();
    program.extend_from_slice(&encrypted);
    program.extend_from_slice(b"\n0000000000000000\ncleartomark\n");
    program
}

fn glyph_font(charstring: &[u8]) -> Vec<u8> {
    // Subrs 0..=3 are the standard flex and hint replacement subroutines; 4 returns at once.
    let subrs: [&[u8]; 5] = [
        &[142, 139, 12, 16, 12, 17, 12, 17, 12, 33, 11],
        &[139, 140, 12, 16, 11],
        &[139, 141, 12, 16, 11],
        &[11],
        &[11],
    ];
    let mut private = PRIVATE_PREFIX.to_vec();
    private.extend_from_slice(format!("/Subrs {} array\n", subrs.len()).as_bytes());
    for (index, subr) in subrs.iter().enumerate() {
        let sealed = seal_charstring(subr);
        private.extend_from_slice(format!("dup {index} {} RD ", sealed.len()).as_bytes());
        private.extend_from_slice(&sealed);
        private.extend_from_slice(b" NP\n");
    }
    private.extend_from_slice(b"ND\n2 index /CharStrings 2 dict dup begin\n");
    for (name, code) in [(&b".notdef"[..], &[139_u8, 247, 108, 13, 14][..]), (b"A", charstring)] {
        let sealed = seal_charstring(code);
        private.extend_from_slice(b"/");
        private.extend_from_slice(name);
        private.extend_from_slice(format!(" {} RD ", sealed.len()).as_bytes());
        private.extend_from_slice(&sealed);
        private.extend_from_slice(b" ND\n");
    }
    private.extend_from_slice(b"end\nend\nreadonly put\nnoaccess put\n");
    wrap_private(&private)
}

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_FUZZ_INPUT {
        return;
    }
    let Some((&mode, rest)) = data.split_first() else {
        return;
    };
    let program = match mode % 3 {
        0 => rest.to_vec(),
        1 => wrap_private(rest),
        _ => glyph_font(rest),
    };
    if let Some(converted) = type1_to_cff(&program, None, MAX_OUTPUT) {
        assert!(converted.bytes.len() <= MAX_OUTPUT);
        assert_eq!(converted.bytes.get(..3), Some(&[1_u8, 0, 4][..]));
    }
});
