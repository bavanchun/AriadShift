//! Byte-deterministic PDF writer (`writer::save`, `writer::save_with_options`).
//!
//! Enforces the determinism and validity contract (`docs/tools.md` §8):
//! 1. Refuses encrypted documents ([`PdfError::Encrypted`]).
//! 2. Garbage-collects unreachable objects ([`crate::gc::collect_document`]).
//! 3. Deduplicates identical streams by SHA-256 digest before saving.
//! 4. Renumbers surviving indirect objects in breadth-first traversal order from the trailer
//!    (`/Root` catalog, `/Info`, etc.) starting at `(1, 0)`.
//! 5. Sorts all dictionary keys lexicographically.
//! 6. Derives `/CreationDate`, `/ModDate`, and trailer `/ID` deterministically from
//!    `SOURCE_DATE_EPOCH` when set, or from the SHA-256 content digest when unset (never reading
//!    the wall clock or OS RNG).
//! 7. Always writes a complete, valid PDF from scratch (no incremental `/Prev` updates), even when
//!    the input document was opened with a repaired cross-reference table.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use lopdf::{Dictionary, Document, Object, ObjectId, Stream, StringFormat};

use crate::{
    document::{PdfDoc, ToolCtx},
    error::PdfError,
    gc,
    naming::INPUT_STEM_TRAILER_KEY,
};

/// Options controlling deterministic PDF serialization.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SaveOptions {
    /// When `true`, keep existing `/CreationDate` and `/ModDate` values in `/Info` (used when
    /// `pdf.metadata` explicitly sets or removes timestamps).
    pub preserve_info_dates: bool,
    /// When `true`, do not create an `/Info` dictionary if the document has none (used when
    /// `pdf.metadata` runs with `strip_all: true`).
    pub omit_info_if_absent: bool,
    /// Optional explicit Unix timestamp in seconds (UTC), overriding the `SOURCE_DATE_EPOCH`
    /// environment variable when `Some`.
    pub epoch_override: Option<i64>,
}

/// Saves `doc` deterministically to PDF bytes using default [`SaveOptions`].
pub fn save(doc: &mut PdfDoc, ctx: &ToolCtx) -> Result<Vec<u8>, PdfError> {
    save_with_options(doc, ctx, &SaveOptions::default())
}

/// Saves `doc` deterministically to PDF bytes with explicit [`SaveOptions`].
pub fn save_with_options(
    doc: &mut PdfDoc,
    ctx: &ToolCtx,
    options: &SaveOptions,
) -> Result<Vec<u8>, PdfError> {
    ctx.check()?;
    let inner = doc.inner_mut()?;
    save_document_with_options(inner, ctx, options)
}

/// Saves a raw [`lopdf::Document`] deterministically to PDF bytes.
pub fn save_document_with_options(
    doc: &mut Document,
    ctx: &ToolCtx,
    options: &SaveOptions,
) -> Result<Vec<u8>, PdfError> {
    ctx.check()?;

    // Strip internal and incremental trailer keys before GC.
    doc.trailer.remove(INPUT_STEM_TRAILER_KEY);
    doc.trailer.remove(b"Prev");
    doc.trailer.remove(b"Encrypt");
    doc.trailer.remove(b"XRefStm");
    doc.trailer.remove(b"Size");
    doc.trailer.remove(b"ID");

    if doc.version.trim().is_empty() {
        doc.version = "1.5".to_owned();
    }

    // 1. Garbage-collect unreachable objects.
    gc::collect_document(doc, ctx)?;

    // 2. Deduplicate identical streams by SHA-256 digest.
    deduplicate_streams(doc, ctx)?;

    // 3. Ensure `/Info` is an indirect object when present or required, and normalize date slots
    //    prior to BFS renumbering and content hashing.
    prepare_info_dictionary(doc, options);

    // 4. Renumber all surviving indirect objects in breadth-first traversal order `(1..=N, 0)`.
    renumber_bfs(doc, ctx)?;

    // 5. Sort all dictionary keys lexicographically and refresh stream `/Length` entries.
    normalize_and_sort_all(doc, ctx)?;

    // 6. Compute canonical SHA-256 content hash over the normalized document.
    let content_hash = compute_document_hash(doc, ctx)?;

    // 7. Derive deterministic `/CreationDate`, `/ModDate`, and `/ID` from `SOURCE_DATE_EPOCH`
    //    (when set) or from `content_hash` (when unset).
    let epoch = options.epoch_override.or_else(read_source_date_epoch_env);
    let (date_bytes, id_digest) = if let Some(epoch_secs) = epoch {
        let date_str = format_pdf_date_utc(epoch_secs);
        let digest = sha256_chunks(&[
            b"ariad-pdf-id-epoch-v1",
            &content_hash,
            &epoch_secs.to_be_bytes(),
        ]);
        (date_str, digest)
    } else {
        let mut head = [0_u8; 8];
        head.copy_from_slice(&content_hash[0..8]);
        let derived_secs = (u64::from_be_bytes(head) % 2_000_000_000) as i64;
        let date_str = format_pdf_date_utc(derived_secs);
        let digest = sha256_chunks(&[b"ariad-pdf-id-content-v1", &content_hash]);
        (date_str, digest)
    };

    if !options.preserve_info_dates {
        stamp_info_dates(doc, &date_bytes);
    }

    doc.trailer.set(
        "ID",
        Object::Array(vec![
            Object::String(id_digest[0..16].to_vec(), StringFormat::Hexadecimal),
            Object::String(id_digest[16..32].to_vec(), StringFormat::Hexadecimal),
        ]),
    );
    sort_dictionary_in_place(&mut doc.trailer);

    ctx.check()?;
    let mut out = Vec::new();
    doc.save_to(&mut out).map_err(|_| PdfError::Malformed)?;
    Ok(out)
}

/// Deduplicates identical streams in `doc` by SHA-256 hash of their canonical dictionary
/// (excluding `/Length`) and raw stream content bytes.
///
/// Returns the total number of duplicate stream objects merged.
pub fn deduplicate_streams(doc: &mut Document, ctx: &ToolCtx) -> Result<usize, PdfError> {
    let mut total_merged = 0_usize;

    // Run up to 4 passes so container streams/dictionaries referencing newly merged streams
    // also converge.
    for _ in 0..4 {
        ctx.check()?;
        let mut sorted_ids: Vec<ObjectId> = doc.objects.keys().copied().collect();
        sorted_ids.sort_unstable();

        let mut canonical_by_hash: HashMap<[u8; 32], ObjectId> = HashMap::new();
        let mut remap: HashMap<ObjectId, ObjectId> = HashMap::new();

        for id in sorted_ids {
            let Some(Object::Stream(stream)) = doc.objects.get(&id) else {
                continue;
            };
            let digest = hash_stream(stream);
            if let Some(&existing_id) = canonical_by_hash.get(&digest) {
                remap.insert(id, existing_id);
            } else {
                canonical_by_hash.insert(digest, id);
            }
        }

        if remap.is_empty() {
            break;
        }

        for dup_id in remap.keys() {
            doc.objects.remove(dup_id);
        }
        for object in doc.objects.values_mut() {
            rewrite_object_references(object, &remap);
        }
        rewrite_dict_references(&mut doc.trailer, &remap);
        total_merged += remap.len();
    }

    doc.max_id = doc.objects.keys().map(|&(num, _)| num).max().unwrap_or(0);
    Ok(total_merged)
}

fn prepare_info_dictionary(doc: &mut Document, options: &SaveOptions) {
    let info_id = match doc.trailer.get(b"Info").cloned() {
        Ok(Object::Reference(id)) if doc.objects.contains_key(&id) => Some(id),
        Ok(Object::Dictionary(dict)) => {
            let id = doc.add_object(Object::Dictionary(dict));
            doc.trailer.set("Info", Object::Reference(id));
            Some(id)
        }
        _ if !options.omit_info_if_absent => {
            let id = doc.add_object(Object::Dictionary(Dictionary::new()));
            doc.trailer.set("Info", Object::Reference(id));
            Some(id)
        }
        _ => {
            doc.trailer.remove(b"Info");
            None
        }
    };

    if !options.preserve_info_dates
        && let Some(id) = info_id
        && let Some(Object::Dictionary(dict)) = doc.objects.get_mut(&id)
    {
        dict.set(
            "CreationDate",
            Object::String(Vec::new(), StringFormat::Literal),
        );
        dict.set("ModDate", Object::String(Vec::new(), StringFormat::Literal));
    }
}

fn stamp_info_dates(doc: &mut Document, date_bytes: &[u8]) {
    if let Ok(&Object::Reference(info_id)) = doc.trailer.get(b"Info")
        && let Some(Object::Dictionary(dict)) = doc.objects.get_mut(&info_id)
    {
        dict.set(
            "CreationDate",
            Object::String(date_bytes.to_vec(), StringFormat::Literal),
        );
        dict.set(
            "ModDate",
            Object::String(date_bytes.to_vec(), StringFormat::Literal),
        );
        sort_dictionary_in_place(dict);
    }
}

/// Renumbers all objects in `doc` in deterministic breadth-first traversal order starting from
/// trailer references (`/Root`, `/Info`, etc.), assigning IDs `(1, 0)..=(N, 0)`.
pub fn renumber_bfs(doc: &mut Document, ctx: &ToolCtx) -> Result<(), PdfError> {
    ctx.check()?;

    let mut queue = VecDeque::new();
    let mut enqueued = HashSet::new();

    // Seed BFS with `/Root` first, then `/Info`, then any remaining trailer references in sorted key order.
    if let Ok(Object::Reference(root_id)) = doc.trailer.get(b"Root")
        && doc.objects.contains_key(root_id)
        && enqueued.insert(*root_id)
    {
        queue.push_back(*root_id);
    }
    if let Ok(Object::Reference(info_id)) = doc.trailer.get(b"Info")
        && doc.objects.contains_key(info_id)
        && enqueued.insert(*info_id)
    {
        queue.push_back(*info_id);
    }

    let mut trailer_keys: Vec<Vec<u8>> = doc.trailer.iter().map(|(k, _)| k.clone()).collect();
    trailer_keys.sort();
    for key in &trailer_keys {
        if let Ok(val) = doc.trailer.get(key) {
            enqueue_refs_sorted(val, &doc.objects, &mut enqueued, &mut queue);
        }
    }

    let mut bfs_order = Vec::with_capacity(doc.objects.len());
    let mut steps = 0_usize;
    while let Some(old_id) = queue.pop_front() {
        steps = steps.wrapping_add(1);
        if steps.is_multiple_of(128) {
            ctx.check()?;
        }
        bfs_order.push(old_id);
        if let Some(obj) = doc.objects.get(&old_id) {
            enqueue_refs_sorted(obj, &doc.objects, &mut enqueued, &mut queue);
        }
    }

    // Include any remaining unvisited objects in ascending `ObjectId` order for completeness.
    if bfs_order.len() < doc.objects.len() {
        let mut remaining: Vec<ObjectId> = doc
            .objects
            .keys()
            .copied()
            .filter(|id| !enqueued.contains(id))
            .collect();
        remaining.sort_unstable();
        bfs_order.extend(remaining);
    }

    let mut remap: HashMap<ObjectId, ObjectId> = HashMap::with_capacity(bfs_order.len());
    for (idx, old_id) in bfs_order.iter().copied().enumerate() {
        let new_num = u32::try_from(idx + 1).map_err(|_| PdfError::Malformed)?;
        remap.insert(old_id, (new_num, 0));
    }

    let mut new_objects = BTreeMap::new();
    for old_id in bfs_order {
        if let Some(mut obj) = doc.objects.remove(&old_id) {
            rewrite_object_references(&mut obj, &remap);
            let new_id = remap[&old_id];
            new_objects.insert(new_id, obj);
        }
    }

    rewrite_dict_references(&mut doc.trailer, &remap);
    doc.max_id = u32::try_from(new_objects.len()).unwrap_or(u32::MAX);
    doc.objects = new_objects;
    Ok(())
}

fn enqueue_refs_sorted(
    object: &Object,
    objects: &BTreeMap<ObjectId, Object>,
    enqueued: &mut HashSet<ObjectId>,
    queue: &mut VecDeque<ObjectId>,
) {
    match object {
        Object::Reference(id) => {
            if objects.contains_key(id) && enqueued.insert(*id) {
                queue.push_back(*id);
            }
        }
        Object::Array(items) => {
            for item in items {
                enqueue_refs_sorted(item, objects, enqueued, queue);
            }
        }
        Object::Dictionary(dict) => {
            let mut keys: Vec<&Vec<u8>> = dict.iter().map(|(k, _)| k).collect();
            keys.sort();
            for key in keys {
                if let Ok(val) = dict.get(key) {
                    enqueue_refs_sorted(val, objects, enqueued, queue);
                }
            }
        }
        Object::Stream(stream) => {
            let mut keys: Vec<&Vec<u8>> = stream.dict.iter().map(|(k, _)| k).collect();
            keys.sort();
            for key in keys {
                if let Ok(val) = stream.dict.get(key) {
                    enqueue_refs_sorted(val, objects, enqueued, queue);
                }
            }
        }
        _ => {}
    }
}

fn rewrite_object_references(object: &mut Object, remap: &HashMap<ObjectId, ObjectId>) {
    match object {
        Object::Reference(id) => {
            if let Some(&new_id) = remap.get(id) {
                *id = new_id;
            }
        }
        Object::Array(items) => {
            for item in items {
                rewrite_object_references(item, remap);
            }
        }
        Object::Dictionary(dict) => {
            rewrite_dict_references(dict, remap);
        }
        Object::Stream(stream) => {
            rewrite_dict_references(&mut stream.dict, remap);
        }
        _ => {}
    }
}

fn rewrite_dict_references(dict: &mut Dictionary, remap: &HashMap<ObjectId, ObjectId>) {
    for (_, val) in dict.iter_mut() {
        rewrite_object_references(val, remap);
    }
}

fn normalize_and_sort_all(doc: &mut Document, ctx: &ToolCtx) -> Result<(), PdfError> {
    let mut steps = 0_usize;
    for object in doc.objects.values_mut() {
        steps = steps.wrapping_add(1);
        if steps.is_multiple_of(128) {
            ctx.check()?;
        }
        normalize_and_sort_object(object);
    }
    sort_dictionary_in_place(&mut doc.trailer);
    Ok(())
}

fn normalize_and_sort_object(object: &mut Object) {
    match object {
        Object::Array(items) => {
            for item in items {
                normalize_and_sort_object(item);
            }
        }
        Object::Dictionary(dict) => {
            for (_, val) in dict.iter_mut() {
                normalize_and_sort_object(val);
            }
            sort_dictionary_in_place(dict);
        }
        Object::Stream(stream) => {
            let len = i64::try_from(stream.content.len()).unwrap_or(i64::MAX);
            stream.dict.set("Length", Object::Integer(len));
            for (_, val) in stream.dict.iter_mut() {
                normalize_and_sort_object(val);
            }
            sort_dictionary_in_place(&mut stream.dict);
        }
        _ => {}
    }
}

fn sort_dictionary_in_place(dict: &mut Dictionary) {
    let mut entries: Vec<(Vec<u8>, Object)> =
        dict.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    entries.sort_by(|(a, _), (b, _)| a.cmp(b));
    let mut sorted = Dictionary::new();
    for (k, v) in entries {
        sorted.set(k, v);
    }
    *dict = sorted;
}

fn hash_stream(stream: &Stream) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"stream-dict:");
    hash_dictionary_canonical(&stream.dict, Some(b"Length"), &mut hasher);
    hasher.update(b":stream-bytes:");
    hasher.update(&(stream.content.len() as u64).to_be_bytes());
    hasher.update(&stream.content);
    hasher.finalize()
}

fn compute_document_hash(doc: &Document, ctx: &ToolCtx) -> Result<[u8; 32], PdfError> {
    let mut hasher = Sha256::new();
    hasher.update(b"pdf-version:");
    hasher.update(doc.version.as_bytes());
    hasher.update(b":objects:");
    let mut steps = 0_usize;
    for (&(num, generation), object) in &doc.objects {
        steps = steps.wrapping_add(1);
        if steps.is_multiple_of(128) {
            ctx.check()?;
        }
        hasher.update(&num.to_be_bytes());
        hasher.update(&generation.to_be_bytes());
        hash_object_canonical(object, &mut hasher);
    }
    hasher.update(b":trailer:");
    hash_dictionary_canonical(&doc.trailer, Some(b"ID"), &mut hasher);
    Ok(hasher.finalize())
}

fn hash_object_canonical(object: &Object, hasher: &mut Sha256) {
    match object {
        Object::Null => hasher.update(b"N"),
        Object::Boolean(b) => hasher.update(if *b { b"B1" } else { b"B0" }),
        Object::Integer(i) => {
            hasher.update(b"I");
            hasher.update(&i.to_be_bytes());
        }
        Object::Real(r) => {
            hasher.update(b"R");
            let canonical_bits = if *r == 0.0 {
                0.0_f32.to_bits()
            } else {
                r.to_bits()
            };
            hasher.update(&canonical_bits.to_be_bytes());
        }
        Object::Name(name) => {
            hasher.update(b"M");
            hasher.update(&(name.len() as u64).to_be_bytes());
            hasher.update(name);
        }
        Object::String(bytes, fmt) => {
            hasher.update(match fmt {
                StringFormat::Literal => b"SL",
                StringFormat::Hexadecimal => b"SH",
            });
            hasher.update(&(bytes.len() as u64).to_be_bytes());
            hasher.update(bytes);
        }
        Object::Array(items) => {
            hasher.update(b"A");
            hasher.update(&(items.len() as u64).to_be_bytes());
            for item in items {
                hash_object_canonical(item, hasher);
            }
        }
        Object::Dictionary(dict) => {
            hasher.update(b"D");
            hash_dictionary_canonical(dict, None, hasher);
        }
        Object::Stream(stream) => {
            hasher.update(b"T");
            hash_dictionary_canonical(&stream.dict, Some(b"Length"), hasher);
            hasher.update(&(stream.content.len() as u64).to_be_bytes());
            hasher.update(&stream.content);
        }
        Object::Reference((num, generation)) => {
            hasher.update(b"P");
            hasher.update(&num.to_be_bytes());
            hasher.update(&generation.to_be_bytes());
        }
    }
}

fn hash_dictionary_canonical(dict: &Dictionary, skip_key: Option<&[u8]>, hasher: &mut Sha256) {
    let mut keys: Vec<&Vec<u8>> = dict
        .iter()
        .map(|(k, _)| k)
        .filter(|k| skip_key != Some(k.as_slice()))
        .collect();
    keys.sort();
    hasher.update(&(keys.len() as u64).to_be_bytes());
    for key in keys {
        hasher.update(&(key.len() as u64).to_be_bytes());
        hasher.update(key);
        if let Ok(val) = dict.get(key) {
            hash_object_canonical(val, hasher);
        }
    }
}

fn read_source_date_epoch_env() -> Option<i64> {
    std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|raw| raw.trim().parse::<i64>().ok())
}

/// Formats a Unix epoch timestamp in seconds as a UTC PDF date string (`D:YYYYMMDDHHmmSSZ`).
#[must_use]
pub fn format_pdf_date_utc(epoch_secs: i64) -> Vec<u8> {
    let clamped = epoch_secs.clamp(0, 253_402_300_799);
    let days = clamped.div_euclid(86_400);
    let secs_of_day = clamped.rem_euclid(86_400);
    let hour = secs_of_day / 3600;
    let minute = (secs_of_day % 3600) / 60;
    let second = secs_of_day % 60;

    // Howard Hinnant's civil_from_days algorithm (days since 1970-01-01 UTC).
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };

    format!("D:{year:04}{m:02}{d:02}{hour:02}{minute:02}{second:02}Z").into_bytes()
}

fn sha256_chunks(chunks: &[&[u8]]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for chunk in chunks {
        hasher.update(chunk);
    }
    hasher.finalize()
}

/// Pure-Rust FIPS 180-4 SHA-256 hasher (WASM-capable, zero external crate dependencies).
struct Sha256 {
    state: [u32; 8],
    buffer: [u8; 64],
    buffer_len: usize,
    total_len: u64,
}

impl Sha256 {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    const fn new() -> Self {
        Self {
            state: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
                0x5be0cd19,
            ],
            buffer: [0_u8; 64],
            buffer_len: 0,
            total_len: 0,
        }
    }

    fn update(&mut self, mut data: &[u8]) {
        self.total_len = self.total_len.wrapping_add(data.len() as u64);
        if self.buffer_len > 0 {
            let needed = 64 - self.buffer_len;
            if data.len() < needed {
                self.buffer[self.buffer_len..self.buffer_len + data.len()].copy_from_slice(data);
                self.buffer_len += data.len();
                return;
            }
            self.buffer[self.buffer_len..64].copy_from_slice(&data[..needed]);
            let block = self.buffer;
            self.compress_block(&block);
            self.buffer_len = 0;
            data = &data[needed..];
        }
        while data.len() >= 64 {
            let mut block = [0_u8; 64];
            block.copy_from_slice(&data[..64]);
            self.compress_block(&block);
            data = &data[64..];
        }
        if !data.is_empty() {
            self.buffer[..data.len()].copy_from_slice(data);
            self.buffer_len = data.len();
        }
    }

    fn finalize(mut self) -> [u8; 32] {
        let bit_len = self.total_len.wrapping_mul(8);
        self.buffer[self.buffer_len] = 0x80;
        self.buffer_len += 1;

        if self.buffer_len > 56 {
            for b in &mut self.buffer[self.buffer_len..64] {
                *b = 0;
            }
            let block = self.buffer;
            self.compress_block(&block);
            self.buffer_len = 0;
        }
        for b in &mut self.buffer[self.buffer_len..56] {
            *b = 0;
        }
        self.buffer[56..64].copy_from_slice(&bit_len.to_be_bytes());
        let final_block = self.buffer;
        self.compress_block(&final_block);

        let mut out = [0_u8; 32];
        for (i, word) in self.state.iter().enumerate() {
            out[i * 4..(i + 1) * 4].copy_from_slice(&word.to_be_bytes());
        }
        out
    }

    fn compress_block(&mut self, block: &[u8; 64]) {
        let mut w = [0_u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                block[i * 4],
                block[i * 4 + 1],
                block[i * 4 + 2],
                block[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let mut a = self.state[0];
        let mut b = self.state[1];
        let mut c = self.state[2];
        let mut d = self.state[3];
        let mut e = self.state[4];
        let mut f = self.state[5];
        let mut g = self.state[6];
        let mut h = self.state[7];

        for (&wi, &ki) in w.iter().zip(Self::K.iter()) {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(ki)
                .wrapping_add(wi);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        self.state[0] = self.state[0].wrapping_add(a);
        self.state[1] = self.state[1].wrapping_add(b);
        self.state[2] = self.state[2].wrapping_add(c);
        self.state[3] = self.state[3].wrapping_add(d);
        self.state[4] = self.state[4].wrapping_add(e);
        self.state[5] = self.state[5].wrapping_add(f);
        self.state[6] = self.state[6].wrapping_add(g);
        self.state[7] = self.state[7].wrapping_add(h);
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use ariad_core::{limits::Limits, reader::pdf as core_pdf};
    use lopdf::dictionary;

    use super::*;
    use crate::document::open;

    #[test]
    fn sha256_matches_nist_vectors_and_utc_date_formatting_is_exact() {
        let empty = sha256_chunks(&[b""]);
        assert_eq!(
            &empty[..4],
            &[0xe3, 0xb0, 0xc4, 0x42],
            "NIST empty vector prefix"
        );
        let abc = sha256_chunks(&[b"abc"]);
        assert_eq!(
            &abc[..4],
            &[0xba, 0x78, 0x16, 0xbf],
            "NIST 'abc' vector prefix"
        );

        assert_eq!(format_pdf_date_utc(0), b"D:19700101000000Z");
        assert_eq!(format_pdf_date_utc(1_700_000_000), b"D:20231114221320Z");
    }

    #[test]
    fn writer_determinism_produces_identical_bytes_with_and_without_source_date_epoch() {
        let fixture_path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pdf-tools/multipage-20.pdf");
        let raw_bytes = std::fs::read(fixture_path).expect("read multipage-20.pdf");

        // 1. Without SOURCE_DATE_EPOCH (content-hash derived dates and /ID):
        let mut doc1 = open(&raw_bytes, &Limits::local()).expect("open doc1");
        let mut doc2 = open(&raw_bytes, &Limits::local()).expect("open doc2");
        let out1 = save_with_options(
            &mut doc1,
            &ToolCtx::default(),
            &SaveOptions {
                epoch_override: None,
                ..SaveOptions::default()
            },
        )
        .expect("save doc1");
        let out2 = save_with_options(
            &mut doc2,
            &ToolCtx::default(),
            &SaveOptions {
                epoch_override: None,
                ..SaveOptions::default()
            },
        )
        .expect("save doc2");
        assert_eq!(
            out1, out2,
            "saving the same input twice without epoch must yield identical bytes"
        );

        // 2. With SOURCE_DATE_EPOCH = 1_700_000_000:
        let mut doc3 = open(&raw_bytes, &Limits::local()).expect("open doc3");
        let mut doc4 = open(&raw_bytes, &Limits::local()).expect("open doc4");
        let epoch_opts = SaveOptions {
            epoch_override: Some(1_700_000_000),
            ..SaveOptions::default()
        };
        let out3 = save_with_options(&mut doc3, &ToolCtx::default(), &epoch_opts)
            .expect("save doc3 with epoch");
        let out4 = save_with_options(&mut doc4, &ToolCtx::default(), &epoch_opts)
            .expect("save doc4 with epoch");
        assert_eq!(
            out3, out4,
            "saving the same input twice with SOURCE_DATE_EPOCH must yield identical bytes"
        );
        assert_ne!(
            out1, out3,
            "SOURCE_DATE_EPOCH must control the stamped /CreationDate, /ModDate, and /ID"
        );

        // Verify the saved output has sequential 1..=N object IDs and expected UTC timestamp.
        let reloaded = open(&out3, &Limits::local()).expect("reload saved pdf");
        let ids: Vec<ObjectId> = reloaded.inner().objects.keys().copied().collect();
        let expected_ids: Vec<ObjectId> = (1..=u32::try_from(ids.len()).unwrap())
            .map(|n| (n, 0))
            .collect();
        assert_eq!(
            ids, expected_ids,
            "objects must be sequentially renumbered 1..=N"
        );

        let info_ref = reloaded
            .inner()
            .trailer
            .get(b"Info")
            .unwrap()
            .as_reference()
            .unwrap();
        let info_dict = reloaded
            .inner()
            .get_object(info_ref)
            .unwrap()
            .as_dict()
            .unwrap();
        assert_eq!(
            info_dict.get(b"CreationDate").unwrap().as_str().unwrap(),
            b"D:20231114221320Z"
        );
        assert_eq!(
            info_dict.get(b"ModDate").unwrap().as_str().unwrap(),
            b"D:20231114221320Z"
        );
    }

    #[test]
    fn writer_deduplicates_identical_streams_and_rewrites_broken_xref_into_valid_pdf() {
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let payload = b"BT /F1 12 Tf 72 720 Td (Duplicate Stream) Tj ET".to_vec();
        let stream_a = doc.add_object(Stream::new(dictionary! {}, payload.clone()));
        let stream_b = doc.add_object(Stream::new(dictionary! {}, payload));
        assert_ne!(stream_a, stream_b);

        let page1 = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Contents" => stream_a,
        });
        let page2 = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Contents" => stream_b,
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![Object::Reference(page1), Object::Reference(page2)],
                "Count" => 2,
            }),
        );
        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        doc.trailer.set("Root", catalog_id);

        let mut raw = Vec::new();
        doc.save_to(&mut raw)
            .expect("save synthetic duplicate-stream pdf");

        let mut pdf_doc = open(&raw, &Limits::local()).expect("open synthetic pdf");
        let saved = save(&mut pdf_doc, &ToolCtx::default()).expect("save deduplicated pdf");
        let reloaded = open(&saved, &Limits::local()).expect("reload deduplicated pdf");

        let stream_count = reloaded
            .inner()
            .objects
            .values()
            .filter(|obj| {
                matches!(
                    obj,
                    Object::Stream(stream) if !stream.dict.has_type(b"XRef")
                )
            })
            .count();
        assert_eq!(
            stream_count, 1,
            "identical page content streams must be deduplicated into a single stream object"
        );

        // Also verify saving `broken-xref.pdf` produces a strictly valid PDF accepted by `ariad_core::reader::pdf::read`.
        let broken_path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pdf-tools/broken-xref.pdf");
        let broken_bytes = std::fs::read(broken_path).expect("read broken-xref.pdf");
        assert!(core_pdf::read(&broken_bytes, &Limits::local()).is_err());

        let mut repaired_doc = open(&broken_bytes, &Limits::local()).expect("open broken-xref.pdf");
        assert!(repaired_doc.repaired_xref());

        let rewritten = save(&mut repaired_doc, &ToolCtx::default())
            .expect("rewrite broken-xref.pdf into valid pdf");
        let reopened = open(&rewritten, &Limits::local()).expect("reopen rewritten pdf");
        assert!(
            !reopened.repaired_xref(),
            "rewritten PDF must have a valid cross-reference table"
        );
        assert!(
            core_pdf::read(&rewritten, &Limits::local()).is_ok(),
            "rewritten PDF must pass strict ariad-core PDF reader"
        );
    }
}
