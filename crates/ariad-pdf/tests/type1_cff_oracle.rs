//! Glyph-by-glyph equality between embedded Type 1 programs and their Type1C conversion.
//!
//! `read-fonts` (an independent Type 1 and CFF implementation) evaluates every glyph of every
//! embedded Type 1 font in the corpus before and after conversion; paths and widths must match.

#![cfg(feature = "codecs")]
use ariad_pdf::{InputDoc, ToolCtx, fonts};
use lopdf::Object;
use read_fonts::{
    ps::{cff::CffFontRef, charstring::CommandSink, type1::Type1Font},
    types::{Fixed, GlyphId},
};
use std::collections::BTreeMap;
#[derive(Default)]
struct R(Vec<String>);
fn f(v: Fixed) -> String {
    format!("{:.3}", v.to_f64())
}
impl CommandSink for R {
    fn move_to(&mut self, x: Fixed, y: Fixed) {
        self.0.push(format!("M{} {}", f(x), f(y)));
    }
    fn line_to(&mut self, x: Fixed, y: Fixed) {
        self.0.push(format!("L{} {}", f(x), f(y)));
    }
    fn curve_to(&mut self, a: Fixed, b: Fixed, c: Fixed, d: Fixed, x: Fixed, y: Fixed) {
        self.0.push(format!(
            "C{} {} {} {} {} {}",
            f(a),
            f(b),
            f(c),
            f(d),
            f(x),
            f(y)
        ));
    }
    fn close(&mut self) {}
}
#[test]
fn converted_corpus_fonts_match_the_type1_oracle() {
    let ctx = ToolCtx::default();
    let mut checked = 0;
    for f_ in [
        "arxiv-multilingual-open-text-2201-05609-v2.pdf",
        "arxiv-tableformer-2203-01017-v1.pdf",
    ] {
        let bytes = std::fs::read(format!(
            "{}/../../fixtures/pdf/{f_}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap();
        let input = InputDoc::new(f_, bytes);
        let doc = input.open_for_mutation(&ctx).unwrap();
        for (id, o) in &doc.inner().objects {
            let Ok(d) = o.as_dict() else { continue };
            if d.get(b"Type").and_then(Object::as_name).ok() != Some(b"FontDescriptor") {
                continue;
            }
            let Ok(ff) = d.get(b"FontFile").and_then(Object::as_reference) else {
                continue;
            };
            let s = doc.inner().get_object(ff).unwrap().as_stream().unwrap();
            let data = s
                .decompressed_content()
                .unwrap_or_else(|_| s.content.clone());
            let l1 = s.dict.get(b"Length1").unwrap().as_i64().unwrap() as usize;
            let l2 = s.dict.get(b"Length2").unwrap().as_i64().unwrap() as usize;
            let conv = fonts::cff::type1_to_cff(&data, Some((l1, l2)), 1 << 26).expect("converted");
            let t1 = Type1Font::new(&data).expect("oracle");
            let cff = CffFontRef::new(&conv.bytes, 0, None).unwrap();
            let sub = cff.subfont(0, &[]).unwrap();
            let mut want: BTreeMap<String, (Option<String>, Vec<String>)> = BTreeMap::new();
            for (gid, name) in t1.glyph_names() {
                let mut r = R::default();
                let w = t1.evaluate_charstring(gid, &mut r).unwrap();
                want.insert(name.to_owned(), (w.map(f), r.0));
            }
            let mut got = BTreeMap::new();
            for (gid, sid) in cff.charset().unwrap().iter() {
                let name = if gid == GlyphId::new(0) {
                    ".notdef".to_string()
                } else {
                    String::from_utf8(
                        sid.resolve_standard()
                            .map_or_else(|_| cff.string(sid).unwrap().to_vec(), <[u8]>::to_vec),
                    )
                    .unwrap()
                };
                let mut r = R::default();
                let w = cff.evaluate_charstring(&sub, gid, &[], &mut r).unwrap();
                got.insert(name, (w.map(f), r.0));
            }
            assert_eq!(want.len(), got.len(), "{id:?} glyph count");
            for (n, v) in &want {
                assert_eq!(got.get(n), Some(v), "{f_} {id:?} glyph {n}");
            }
            checked += want.len();
        }
    }
    assert!(checked > 0, "the corpus must contain Type 1 fonts");
}

mod hook {
    use ariad_pdf::{InputDoc, PdfDoc, ToolCtx, fonts, optimize};
    use lopdf::{Dictionary, Object, ObjectId};

    const FIXTURE: &str = "arxiv-multilingual-open-text-2201-05609-v2.pdf";

    fn open() -> (PdfDoc, InputDoc) {
        let bytes = std::fs::read(format!(
            "{}/../../fixtures/pdf/{FIXTURE}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap();
        let input = InputDoc::new(FIXTURE, bytes);
        let doc = input.open_for_mutation(&ToolCtx::default()).unwrap();
        (doc, input)
    }

    fn type1_fonts(doc: &PdfDoc) -> Vec<ObjectId> {
        doc.inner()
            .objects
            .iter()
            .filter(|(_, o)| {
                o.as_dict()
                    .ok()
                    .and_then(|d| d.get(b"Subtype").and_then(Object::as_name).ok())
                    == Some(b"Type1".as_slice())
                    && o.as_dict().is_ok_and(|d| d.has(b"FontDescriptor"))
            })
            .map(|(id, _)| *id)
            .collect()
    }

    fn descriptor(doc: &PdfDoc, font: ObjectId) -> Dictionary {
        let font = doc.inner().get_object(font).unwrap().as_dict().unwrap();
        let id = font.get(b"FontDescriptor").unwrap().as_reference().unwrap();
        doc.inner().get_dictionary(id).unwrap().clone()
    }

    #[test]
    fn conversion_swaps_only_the_font_program() {
        let (mut doc, input) = open();
        let fonts_before = type1_fonts(&doc);
        assert_eq!(fonts_before.len(), 6);
        let before: Vec<Dictionary> = fonts_before.iter().map(|f| descriptor(&doc, *f)).collect();
        let dicts: Vec<Dictionary> = fonts_before
            .iter()
            .map(|f| {
                doc.inner()
                    .get_object(*f)
                    .unwrap()
                    .as_dict()
                    .unwrap()
                    .clone()
            })
            .collect();
        let count =
            optimize::convert_type1_fonts(&mut doc, &input.limits, &ToolCtx::default()).unwrap();
        assert_eq!(count, 6);
        for ((font, old), old_font) in fonts_before.iter().zip(&before).zip(&dicts) {
            let new = descriptor(&doc, *font);
            assert!(!new.has(b"FontFile"));
            let program = new.get(b"FontFile3").unwrap().as_reference().unwrap();
            let stream = doc
                .inner()
                .get_object(program)
                .unwrap()
                .as_stream()
                .unwrap();
            assert_eq!(
                stream
                    .dict
                    .get(b"Subtype")
                    .and_then(Object::as_name)
                    .unwrap(),
                b"Type1C"
            );
            for key in [b"Length1".as_slice(), b"Length2", b"Length3"] {
                assert!(!stream.dict.has(key));
            }
            let flat = stream.decompressed_content().unwrap();
            assert_eq!(&flat[..3], &[1, 0, 4]);
            // Every other descriptor key survives unchanged.
            for (key, value) in old {
                if key.as_slice() != b"FontFile" {
                    assert_eq!(
                        new.get(key).unwrap(),
                        value,
                        "{}",
                        String::from_utf8_lossy(key)
                    );
                }
            }
            // The font dictionary itself only changes its descriptor reference.
            let new_font = doc.inner().get_object(*font).unwrap().as_dict().unwrap();
            for (key, value) in old_font {
                if key.as_slice() != b"FontDescriptor" {
                    assert_eq!(new_font.get(key).unwrap(), value);
                }
            }
        }
    }

    #[test]
    fn corrupt_programs_are_left_byte_identical() {
        let (mut doc, input) = open();
        let font = type1_fonts(&doc)[0];
        let program_id = descriptor(&doc, font)
            .get(b"FontFile")
            .unwrap()
            .as_reference()
            .unwrap();
        // Destroy the eexec part while keeping the declared lengths valid.
        let stream = doc
            .inner_mut()
            .unwrap()
            .get_object_mut(program_id)
            .unwrap()
            .as_stream_mut()
            .unwrap();
        let mut plain = stream.decompressed_content().unwrap();
        let l1 = stream.dict.get(b"Length1").unwrap().as_i64().unwrap() as usize;
        for byte in &mut plain[l1 + 8..l1 + 200] {
            *byte ^= 0xa5;
        }
        stream.set_plain_content(plain);
        let snapshot = doc.inner().clone();
        let outcome = fonts::convert_simple_type1_to_cff(&mut doc, font, &input.limits).unwrap();
        assert!(
            matches!(outcome, fonts::SubsetOutcome::Skipped(_)),
            "{outcome:?}"
        );
        assert_eq!(
            doc.inner().objects,
            snapshot.objects,
            "refused font must not change"
        );
    }

    #[test]
    fn shared_descriptors_are_skipped() {
        let (mut doc, input) = open();
        let fonts_with = type1_fonts(&doc);
        let (first, second) = (fonts_with[0], fonts_with[1]);
        let shared = doc
            .inner()
            .get_object(first)
            .unwrap()
            .as_dict()
            .unwrap()
            .get(b"FontDescriptor")
            .unwrap()
            .clone();
        doc.inner_mut()
            .unwrap()
            .get_object_mut(second)
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("FontDescriptor", shared);
        let snapshot = doc.inner().objects.clone();
        for font in [first, second] {
            let outcome =
                fonts::convert_simple_type1_to_cff(&mut doc, font, &input.limits).unwrap();
            assert_eq!(
                outcome,
                fonts::SubsetOutcome::Skipped("shared font program")
            );
        }
        assert_eq!(doc.inner().objects, snapshot);
    }
}
