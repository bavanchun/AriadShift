//! Fuzzes the Type 1 to Type1C (CFF) converter (`ariad_pdf::fonts::cff::type1_to_cff`).
//!
//! The first input byte selects what the rest means, so mutation reaches every parser layer:
//! - 0: the whole program, passed through untouched (header and eexec detection).
//! - 1: the decrypted private part, wrapped in a valid cleartext header and eexec-encrypted
//!   (Subrs and CharStrings parsing).
//! - 2: one glyph charstring inside an otherwise valid font with the standard flex and hint
//!   replacement subroutines (the Type 1 interpreter and the Type 2 encoder).
//! - 3: as 1, but with the exact `Some((length1, length2))` section lengths a PDF carries.
//! - 4: the program after four length bytes, with those bytes read as `Some((length1,
//!   length2))` (the explicit path with lengths that may be wrong).
//!
//! Invariants: the converter neither panics nor allocates past its limit, and any accepted
//! program yields a CFF that has the 1.0 header, stays within the limit and parses with
//! `read-fonts`. Its glyph count and charset names equal the input's, as read by the independent
//! `read-fonts` Type 1 parser (plus the `.notdef` the converter adds when the input has none).
#![no_main]

use ariad_pdf::fonts::cff::{Converted, type1_to_cff};
use libfuzzer_sys::fuzz_target;
use read_fonts::{
    ps::{cff::CffFontRef, type1::Type1Font},
    types::GlyphId,
};

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

/// A program and the `/Length1`, `/Length2` values a PDF would carry for it.
struct Program {
    bytes: Vec<u8>,
    lengths: Option<(usize, usize)>,
    /// Whether the lengths are absent or exact, so an independent parser sees the same sections.
    oracle: bool,
}

fn wrap_private(private: &[u8]) -> Program {
    let mut plain = vec![0x77, 0x31, 0x42, 0x53];
    plain.extend_from_slice(private);
    let encrypted = crypt(&plain, 55665, true);
    let mut bytes = HEAD.to_vec();
    bytes.extend_from_slice(&encrypted);
    bytes.extend_from_slice(b"\n0000000000000000\ncleartomark\n");
    Program {
        bytes,
        lengths: Some((HEAD.len(), encrypted.len())),
        oracle: true,
    }
}

fn glyph_font(charstring: &[u8]) -> Program {
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
    for (name, code) in [
        (&b".notdef"[..], &[139_u8, 247, 108, 13, 14][..]),
        (b"A", charstring),
    ] {
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

/// Check an accepted conversion against the input, read by independent parsers.
///
/// `oracle` is false when the section lengths were arbitrary, so the converter may have read a
/// different region than a parser that finds the eexec section itself.
fn check(program: &[u8], converted: &Converted, oracle: bool) {
    assert!(converted.bytes.len() <= MAX_OUTPUT);
    assert_eq!(converted.bytes.get(..3), Some(&[1_u8, 0, 4][..]));
    let cff = CffFontRef::new(&converted.bytes, 0, None).expect("accepted output parses as CFF");
    let charset = cff.charset().expect("accepted output has a charset");
    let mut names: Vec<Vec<u8>> = Vec::new();
    for (gid, sid) in charset.iter() {
        names.push(if gid == GlyphId::new(0) {
            b".notdef".to_vec()
        } else {
            sid.resolve_standard().map_or_else(
                |_| cff.string(sid).map_or_else(Vec::new, <[u8]>::to_vec),
                <[u8]>::to_vec,
            )
        });
    }
    assert_eq!(names.first().map(Vec::as_slice), Some(&b".notdef"[..]));
    assert!(
        names.iter().skip(1).all(|name| name != b".notdef"),
        "the charset lists .notdef once"
    );
    // `glyphs` counts the glyphs read from the input, without a `.notdef` the converter adds.
    assert!(
        names.len() == converted.glyphs || names.len() == converted.glyphs + 1,
        "glyph count {} vs converter {}",
        names.len(),
        converted.glyphs
    );
    if !oracle {
        return;
    }
    // Independent view of the input: when `read-fonts` reads it, glyph count and names agree.
    if let Ok(input) = Type1Font::new(program) {
        let mut want: Vec<Vec<u8>> = input
            .glyph_names()
            .map(|(_, name)| name.as_bytes().to_vec())
            .collect();
        // The converter adds a `.notdef` when the program defines none, and `read-fonts`
        // lists one in that case too; compare the names, which also fixes the glyph count.
        if !want.iter().any(|name| name == b".notdef") {
            want.push(b".notdef".to_vec());
        }
        want.sort();
        names.sort();
        assert_eq!(names, want, "charset names differ from the input's");
    }
}

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_FUZZ_INPUT {
        return;
    }
    let Some((&mode, rest)) = data.split_first() else {
        return;
    };
    let program = match mode % 5 {
        0 => Program {
            bytes: rest.to_vec(),
            lengths: None,
            oracle: true,
        },
        1 => Program {
            lengths: None,
            ..wrap_private(rest)
        },
        2 => Program {
            lengths: None,
            ..glyph_font(rest)
        },
        3 => wrap_private(rest),
        _ => {
            let Some((lengths, program)) = rest.split_first_chunk::<4>() else {
                return;
            };
            Program {
                bytes: program.to_vec(),
                lengths: Some((
                    usize::from(u16::from_be_bytes([lengths[0], lengths[1]])),
                    usize::from(u16::from_be_bytes([lengths[2], lengths[3]])),
                )),
                oracle: false,
            }
        }
    };
    if let Some(converted) = type1_to_cff(&program.bytes, program.lengths, MAX_OUTPUT) {
        check(&program.bytes, &converted, program.oracle);
    }
});
