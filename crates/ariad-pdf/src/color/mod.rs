//! Reusable PDF colour-operator rewriting.

use crate::{PdfError, ToolCtx, document::DecodeBudget};
use ariad_core::limits::Limits;
use lopdf::{Dictionary, Document, Object, content::Content};

mod document;
mod inline;
mod resources;
#[cfg(feature = "codecs")]
pub(crate) mod targeted;
#[cfg(feature = "codecs")]
pub(crate) mod targeted_document;

pub use document::rewrite_document;

pub(crate) fn page_ids(
    doc: &Document,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<Vec<lopdf::ObjectId>, PdfError> {
    const MAX_PAGE_NODES: usize = 100_000;
    let root = doc
        .catalog()
        .and_then(|d| d.get(b"Pages"))
        .and_then(Object::as_reference)
        .map_err(|_| PdfError::Malformed)?;
    let mut stack = vec![(root, 0_u16)];
    let mut seen = std::collections::HashSet::new();
    let mut pages = Vec::new();
    while let Some((id, depth)) = stack.pop() {
        ctx.check()?;
        if depth > limits.max_nesting_depth {
            return Err(PdfError::LimitExceeded(
                ariad_core::reader::pdf::ReadError::NestingTooDeep {
                    limit: limits.max_nesting_depth,
                },
            ));
        }
        if !seen.insert(id) {
            return Err(PdfError::Malformed);
        }
        if seen.len() > MAX_PAGE_NODES {
            return Err(PdfError::LimitExceeded(
                ariad_core::reader::pdf::ReadError::TooManyObjects {
                    limit: MAX_PAGE_NODES,
                },
            ));
        }
        let node = doc
            .get_object(id)
            .and_then(Object::as_dict)
            .map_err(|_| PdfError::Malformed)?;
        match node
            .get(b"Type")
            .and_then(Object::as_name)
            .map_err(|_| PdfError::Malformed)?
        {
            b"Page" => {
                pages.push(id);
                let max_pages = match (limits.max_pages, ctx.max_pages) {
                    (Some(a), Some(b)) => Some(a.min(b)),
                    (a, None) | (None, a) => a,
                };
                if let Some(max) = max_pages
                    && pages.len() > max as usize
                {
                    return Err(PdfError::LimitExceeded(
                        ariad_core::reader::pdf::ReadError::TooManyPages {
                            actual: pages.len() as u64,
                            limit: max,
                        },
                    ));
                }
            }
            b"Pages" => {
                let mut kids = node.get(b"Kids").map_err(|_| PdfError::Malformed)?;
                let mut refs = 0_u16;
                while let Object::Reference(child) = kids {
                    if refs >= limits.max_nesting_depth {
                        return Err(PdfError::LimitExceeded(
                            ariad_core::reader::pdf::ReadError::NestingTooDeep {
                                limit: limits.max_nesting_depth,
                            },
                        ));
                    }
                    refs += 1;
                    kids = doc.get_object(*child).map_err(|_| PdfError::Malformed)?;
                }
                let children = kids.as_array().map_err(|_| PdfError::Malformed)?;
                if children
                    .len()
                    .saturating_add(stack.len())
                    .saturating_add(seen.len())
                    > MAX_PAGE_NODES
                {
                    return Err(PdfError::LimitExceeded(
                        ariad_core::reader::pdf::ReadError::TooManyObjects {
                            limit: MAX_PAGE_NODES,
                        },
                    ));
                }
                for child in children.iter().rev() {
                    stack.push((
                        child.as_reference().map_err(|_| PdfError::Malformed)?,
                        depth.saturating_add(1),
                    ));
                }
            }
            _ => return Err(PdfError::Malformed),
        }
    }
    if pages.is_empty() {
        return Err(PdfError::Malformed);
    }
    Ok(pages)
}

pub(crate) fn page_resources(
    doc: &Document,
    page: lopdf::ObjectId,
    limits: &Limits,
) -> Result<Dictionary, PdfError> {
    let mut current = Some(page);
    let mut seen = std::collections::HashSet::new();
    let mut depth = 0_u16;
    while let Some(id) = current {
        if depth > limits.max_nesting_depth || depth >= 64 {
            return Err(PdfError::LimitExceeded(
                ariad_core::reader::pdf::ReadError::NestingTooDeep {
                    limit: limits.max_nesting_depth.min(63),
                },
            ));
        }
        if !seen.insert(id) {
            return Err(PdfError::Malformed);
        }
        let dict = doc
            .get_object(id)
            .and_then(Object::as_dict)
            .map_err(|_| PdfError::Malformed)?;
        current = match dict.get(b"Parent") {
            Ok(value) => Some(value.as_reference().map_err(|_| PdfError::Malformed)?),
            Err(_) => None,
        };
        depth = depth.saturating_add(1);
    }
    Ok(crate::xobject::resolve_page_resources(doc, page))
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum ActiveSpace {
    Device(ColorSpace),
    Indexed,
    Pattern(ColorSpace),
}

struct ColorContext<'a> {
    limits: &'a Limits,
    ctx: &'a ToolCtx,
    budget: &'a mut DecodeBudget,
}

/// Device colour models supported by the mapper.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ColorSpace {
    Gray,
    Rgb,
    Cmyk,
}

impl ColorSpace {
    const fn count(self) -> usize {
        match self {
            Self::Gray => 1,
            Self::Rgb => 3,
            Self::Cmyk => 4,
        }
    }
    const fn name(self) -> &'static [u8] {
        match self {
            Self::Gray => b"DeviceGray",
            Self::Rgb => b"DeviceRGB",
            Self::Cmyk => b"DeviceCMYK",
        }
    }
    const fn operator(self, stroke: bool) -> &'static str {
        match (self, stroke) {
            (Self::Gray, false) => "g",
            (Self::Gray, true) => "G",
            (Self::Rgb, false) => "rg",
            (Self::Rgb, true) => "RG",
            (Self::Cmyk, false) => "k",
            (Self::Cmyk, true) => "K",
        }
    }
}

/// A colour after mapping to a device colour space.
#[derive(Clone, Debug, PartialEq)]
pub struct MappedColor {
    pub space: ColorSpace,
    pub components: Vec<f32>,
}

/// An operator that was preserved because its colour cannot safely be converted.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnconvertedColor {
    pub operator: String,
    pub reason: &'static str,
}

/// Create a mapper from device colours to luminance.
pub fn to_gray_mapper() -> impl Fn(ColorSpace, &[f32]) -> Option<MappedColor> {
    |space, c| {
        if c.len() != space.count() {
            return None;
        }
        let (r, g, b) = match space {
            ColorSpace::Gray => (c[0], c[0], c[0]),
            ColorSpace::Rgb => (c[0], c[1], c[2]),
            ColorSpace::Cmyk => (
                (1.0 - c[0]).clamp(0.0, 1.0) * (1.0 - c[3]).clamp(0.0, 1.0),
                (1.0 - c[1]).clamp(0.0, 1.0) * (1.0 - c[3]).clamp(0.0, 1.0),
                (1.0 - c[2]).clamp(0.0, 1.0) * (1.0 - c[3]).clamp(0.0, 1.0),
            ),
        };
        Some(MappedColor {
            space: ColorSpace::Gray,
            components: vec![(0.299 * r + 0.587 * g + 0.114 * b).clamp(0.0, 1.0)],
        })
    }
}
pub(crate) fn resolve<'a>(
    doc: &'a Document,
    mut obj: &'a Object,
    limit: u16,
) -> Option<&'a Object> {
    for _ in 0..=limit {
        match obj {
            Object::Reference(id) => obj = doc.get_object(*id).ok()?,
            _ => return Some(obj),
        }
    }
    None
}
fn dict<'a>(doc: &'a Document, obj: &'a Object, limit: u16) -> Option<&'a Dictionary> {
    resolve(doc, obj, limit)?.as_dict().ok()
}
fn space(doc: &Document, resources: &Dictionary, name: &[u8], limit: u16) -> Option<ColorSpace> {
    match name {
        b"DeviceGray" | b"G" => return Some(ColorSpace::Gray),
        b"DeviceRGB" | b"RGB" => return Some(ColorSpace::Rgb),
        b"DeviceCMYK" | b"CMYK" => return Some(ColorSpace::Cmyk),
        _ => {}
    }
    let entry = resources
        .get(b"ColorSpace")
        .ok()
        .and_then(|v| dict(doc, v, limit))?
        .get(name)
        .ok()?;
    let array = resolve(doc, entry, limit)?.as_array().ok()?;
    if array.first()?.as_name().ok()? != b"ICCBased" {
        return None;
    }
    let stream = resolve(doc, array.get(1)?, limit)?.as_stream().ok()?;
    match stream.dict.get(b"N").and_then(Object::as_i64).ok()? {
        1 => Some(ColorSpace::Gray),
        3 => Some(ColorSpace::Rgb),
        4 => Some(ColorSpace::Cmyk),
        _ => None,
    }
}
fn scalar(o: &Object) -> Option<f32> {
    match o {
        Object::Integer(v) => Some(*v as f32),
        Object::Real(v) => Some(*v),
        _ => None,
    }
}

fn mapper_accepts_content<F>(
    doc: &Document,
    content: &Content,
    resources: &Dictionary,
    special: &std::collections::BTreeMap<Vec<u8>, resources::Special>,
    mapper: &F,
    initial: (Option<ActiveSpace>, Option<ActiveSpace>),
    limit: u16,
) -> bool
where
    F: Fn(ColorSpace, &[f32]) -> Option<MappedColor>,
{
    let (mut fill, mut stroke) = initial;
    let mut stack = Vec::new();
    for op in &content.operations {
        let name = op.operator.as_str();
        match name {
            "q" => {
                stack.push((fill, stroke));
                continue;
            }
            "Q" => {
                if let Some((f, s)) = stack.pop() {
                    fill = f;
                    stroke = s;
                }
                continue;
            }
            "cs" | "CS" => {
                let current = op
                    .operands
                    .first()
                    .and_then(|o| o.as_name().ok())
                    .and_then(|n| match special.get(n) {
                        Some(resources::Special::Indexed) => Some(ActiveSpace::Indexed),
                        Some(resources::Special::Pattern(s)) => Some(ActiveSpace::Pattern(*s)),
                        None => space(doc, resources, n, limit).map(ActiveSpace::Device),
                    });
                if name == "cs" {
                    fill = current
                } else {
                    stroke = current
                };
                continue;
            }
            _ => {}
        }
        let current = match name {
            "g" => {
                fill = Some(ActiveSpace::Device(ColorSpace::Gray));
                fill
            }
            "G" => {
                stroke = Some(ActiveSpace::Device(ColorSpace::Gray));
                stroke
            }
            "rg" => {
                fill = Some(ActiveSpace::Device(ColorSpace::Rgb));
                fill
            }
            "RG" => {
                stroke = Some(ActiveSpace::Device(ColorSpace::Rgb));
                stroke
            }
            "k" => {
                fill = Some(ActiveSpace::Device(ColorSpace::Cmyk));
                fill
            }
            "K" => {
                stroke = Some(ActiveSpace::Device(ColorSpace::Cmyk));
                stroke
            }
            "sc" | "scn" => fill,
            "SC" | "SCN" => stroke,
            _ => continue,
        };
        let (source, pattern) = match current {
            Some(ActiveSpace::Device(s)) => (s, false),
            Some(ActiveSpace::Pattern(s)) => (s, true),
            _ => continue,
        };
        let operands = if pattern {
            match op.operands.split_last() {
                Some((Object::Name(_), v)) => v,
                _ => continue,
            }
        } else {
            op.operands.as_slice()
        };
        let Some(values) = operands.iter().map(scalar).collect::<Option<Vec<_>>>() else {
            continue;
        };
        if values.len() != source.count() {
            continue;
        }
        let Some(mapped) = mapper(source, &values) else {
            return false;
        };
        if mapped.components.len() != mapped.space.count()
            || mapped.components.iter().any(|v| !v.is_finite())
        {
            return false;
        }
        if pattern
            && mapper(source, &vec![0.0; source.count()])
                .is_none_or(|zero| zero.space != mapped.space)
        {
            return false;
        }
    }
    true
}

/// Rewrite colour operators in a single parsed content stream and return preserved cases.
/// The mapper must consistently return the same output colour space for a given input space.
pub fn rewrite_content<F>(
    doc: &Document,
    content: &mut Content,
    resources: &mut Dictionary,
    mapper: &F,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<Vec<UnconvertedColor>, PdfError>
where
    F: Fn(ColorSpace, &[f32]) -> Option<MappedColor>,
{
    let mut budget = DecodeBudget::from_limits(usize::MAX, limits);
    let mut context = ColorContext {
        limits,
        ctx,
        budget: &mut budget,
    };
    rewrite_content_with_state(
        doc,
        content,
        resources,
        mapper,
        &mut context,
        (
            Some(ActiveSpace::Device(ColorSpace::Gray)),
            Some(ActiveSpace::Device(ColorSpace::Gray)),
        ),
    )
}

fn rewrite_content_with_state<F>(
    doc: &Document,
    content: &mut Content,
    resources: &mut Dictionary,
    mapper: &F,
    context: &mut ColorContext<'_>,
    initial: (Option<ActiveSpace>, Option<ActiveSpace>),
) -> Result<Vec<UnconvertedColor>, PdfError>
where
    F: Fn(ColorSpace, &[f32]) -> Option<MappedColor>,
{
    let limits = context.limits;
    context.ctx.check()?;
    let (converted_resources, special) =
        resources::convert(doc, resources, mapper, limits, context.budget)?;
    if !mapper_accepts_content(
        doc,
        content,
        resources,
        &special,
        mapper,
        initial,
        limits.max_nesting_depth,
    ) {
        return Ok(vec![UnconvertedColor {
            operator: "colour".into(),
            reason: "mapper declined or returned inconsistent colour",
        }]);
    }
    let mut warnings = Vec::new();
    if let Some(fonts) = resources
        .get(b"Font")
        .ok()
        .and_then(|v| dict(doc, v, limits.max_nesting_depth))
    {
        for (_, font) in fonts {
            if dict(doc, font, limits.max_nesting_depth).is_some_and(|d| {
                d.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Type3".as_slice())
            }) {
                warnings.push(UnconvertedColor {
                    operator: "d0".into(),
                    reason: "Type3 glyph colours are not rewritten",
                });
            }
        }
    }
    let (mut fill, mut stroke) = initial;
    let mut stack = Vec::new();
    for op in &mut content.operations {
        let opname = op.operator.clone();
        if opname == "BI" {
            if let [Object::Stream(image)] = op.operands.as_mut_slice() {
                match inline::rewrite(
                    doc,
                    image,
                    resources,
                    mapper,
                    crate::document::image_memory_limit(limits),
                    limits.max_nesting_depth,
                    context.budget,
                ) {
                    Ok(()) => continue,
                    Err(inline::InlineError::Limit) => {
                        return Err(PdfError::LimitExceeded(
                            ariad_core::reader::pdf::ReadError::DecompressedTooLarge {
                                limit: crate::document::image_memory_limit(limits),
                            },
                        ));
                    }
                    Err(inline::InlineError::Budget(error)) => return Err(error),
                    Err(inline::InlineError::Unsupported) => {}
                }
            }
            warnings.push(UnconvertedColor {
                operator: opname,
                reason: "unsupported inline image",
            });
            continue;
        }
        if matches!(opname.as_str(), "sh" | "d0") {
            warnings.push(UnconvertedColor {
                operator: opname,
                reason: "colour-bearing operation requires additional handling",
            });
            continue;
        }
        if opname == "q" {
            stack.push((fill, stroke));
            continue;
        }
        if opname == "Q" {
            if let Some((f, s)) = stack.pop() {
                fill = f;
                stroke = s;
            }
            continue;
        }
        let stroke_op = matches!(opname.as_str(), "G" | "RG" | "K" | "CS" | "SC" | "SCN");
        if opname == "cs" || opname == "CS" {
            let current = op
                .operands
                .first()
                .and_then(|o| o.as_name().ok())
                .and_then(|n| match special.get(n) {
                    Some(resources::Special::Indexed) => Some(ActiveSpace::Indexed),
                    Some(resources::Special::Pattern(base)) => Some(ActiveSpace::Pattern(*base)),
                    None => {
                        space(doc, resources, n, limits.max_nesting_depth).map(ActiveSpace::Device)
                    }
                });
            if stroke_op {
                stroke = current
            } else {
                fill = current
            }
            if matches!(
                current,
                Some(ActiveSpace::Indexed | ActiveSpace::Pattern(_))
            ) {
                continue;
            }
            if let Some(ActiveSpace::Device(source)) = current
                && let Some(mapped) = mapper(source, &vec![0.0; source.count()])
                && mapped.components.len() == mapped.space.count()
            {
                op.operands = vec![Object::Name(mapped.space.name().to_vec())];
                continue;
            }
            warnings.push(UnconvertedColor {
                operator: opname,
                reason: "unsupported colour space",
            });
            continue;
        }
        let active = match opname.as_str() {
            "g" | "G" => Some(ActiveSpace::Device(ColorSpace::Gray)),
            "rg" | "RG" => Some(ActiveSpace::Device(ColorSpace::Rgb)),
            "k" | "K" => Some(ActiveSpace::Device(ColorSpace::Cmyk)),
            "sc" | "scn" => fill,
            "SC" | "SCN" => stroke,
            _ => continue,
        };
        let Some(active) = active else {
            warnings.push(UnconvertedColor {
                operator: opname,
                reason: "unsupported colour space",
            });
            continue;
        };
        if matches!(active, ActiveSpace::Indexed) {
            continue;
        }
        let (source, pattern) = match active {
            ActiveSpace::Device(s) => (s, false),
            ActiveSpace::Pattern(s) => (s, true),
            ActiveSpace::Indexed => unreachable!(),
        };
        let (components, pattern_name) = if pattern {
            match op.operands.split_last() {
                Some((Object::Name(name), components)) => (components, Some(name.clone())),
                _ => {
                    warnings.push(UnconvertedColor {
                        operator: opname,
                        reason: "invalid pattern colour",
                    });
                    continue;
                }
            }
        } else {
            (op.operands.as_slice(), None)
        };
        let Some(values) = components.iter().map(scalar).collect::<Option<Vec<_>>>() else {
            warnings.push(UnconvertedColor {
                operator: opname,
                reason: "non-device colour operands",
            });
            continue;
        };
        if values.len() != source.count() {
            warnings.push(UnconvertedColor {
                operator: opname,
                reason: "invalid colour operands",
            });
            continue;
        }
        let Some(mapped) = mapper(source, &values) else {
            warnings.push(UnconvertedColor {
                operator: opname,
                reason: "mapper declined colour",
            });
            continue;
        };
        if mapped.components.len() != mapped.space.count()
            || mapped.components.iter().any(|v| !v.is_finite())
        {
            warnings.push(UnconvertedColor {
                operator: opname,
                reason: "invalid mapped colour",
            });
            continue;
        }
        if !pattern {
            op.operator = mapped.space.operator(stroke_op).into();
        }
        op.operands = mapped.components.into_iter().map(Object::Real).collect();
        if let Some(name) = pattern_name {
            op.operands.push(Object::Name(name));
        }
        if matches!(opname.as_str(), "g" | "rg" | "k") {
            fill = Some(ActiveSpace::Device(source))
        }
        if matches!(opname.as_str(), "G" | "RG" | "K") {
            stroke = Some(ActiveSpace::Device(source))
        }
    }
    *resources = converted_resources;
    Ok(warnings)
}

fn numbers(object: &Object) -> Option<Vec<f32>> {
    let values: Vec<f32> = object
        .as_array()
        .ok()?
        .iter()
        .map(scalar)
        .collect::<Option<_>>()?;
    values.iter().all(|x| x.is_finite()).then_some(values)
}
fn numbers_or_default(dict: &Dictionary, key: &[u8], default: &[f32]) -> Option<Vec<f32>> {
    match dict.get(key) {
        Ok(value) => numbers(value),
        Err(_) => Some(default.to_vec()),
    }
}

fn sample_function(
    doc: &Document,
    value: &Object,
    x: f32,
    depth: u16,
    limit: u16,
) -> Option<Vec<f32>> {
    if depth > limit {
        return None;
    }
    let function = dict(doc, value, limit.saturating_sub(depth))?;
    let domain = numbers_or_default(function, b"Domain", &[0.0, 1.0])?;
    if domain.len() != 2 || domain[1] <= domain[0] {
        return None;
    }
    let x = x.clamp(domain[0], domain[1]);
    let mut out = match function
        .get(b"FunctionType")
        .and_then(Object::as_i64)
        .ok()?
    {
        2 => {
            let c0 = numbers_or_default(function, b"C0", &[0.0])?;
            let c1 = numbers_or_default(function, b"C1", &[1.0])?;
            if c0.is_empty() || c0.len() != c1.len() {
                return None;
            }
            let n = function.get(b"N").ok().and_then(scalar)?;
            let factor = x.powf(n);
            c0.iter()
                .zip(c1.iter())
                .map(|(a, b)| a + factor * (b - a))
                .collect::<Vec<_>>()
        }
        3 => {
            let functions = function.get(b"Functions").ok()?.as_array().ok()?;
            let bounds = function.get(b"Bounds").ok().and_then(numbers)?;
            let encode = function.get(b"Encode").ok().and_then(numbers)?;
            if functions.is_empty()
                || bounds.len() + 1 != functions.len()
                || encode.len() != functions.len() * 2
            {
                return None;
            }
            if bounds.windows(2).any(|p| p[0] >= p[1])
                || bounds.iter().any(|&b| b <= domain[0] || b >= domain[1])
            {
                return None;
            }
            let index = bounds.iter().take_while(|&&bound| x >= bound).count();
            let lo = if index == 0 {
                domain[0]
            } else {
                bounds[index - 1]
            };
            let hi = if index == bounds.len() {
                domain[1]
            } else {
                bounds[index]
            };
            if hi <= lo {
                return None;
            }
            let mapped = encode[index * 2]
                + (x - lo) / (hi - lo) * (encode[index * 2 + 1] - encode[index * 2]);
            sample_function(
                doc,
                &functions[index],
                mapped,
                depth.saturating_add(1),
                limit,
            )?
        }
        _ => return None,
    };
    if let Ok(value) = function.get(b"Range") {
        let range = numbers(value)?;
        if range.len() != out.len() * 2 {
            return None;
        }
        for (i, value) in out.iter_mut().enumerate() {
            if range[i * 2] > range[i * 2 + 1] {
                return None;
            };
            *value = value.clamp(range[i * 2], range[i * 2 + 1]);
        }
    }
    out.iter().all(|x| x.is_finite()).then_some(out)
}

/// Sample an axial or radial shading function into a mapped type-0 function.
/// Unsupported function kinds and colour spaces are left unchanged.
pub fn rewrite_shading<F>(
    doc: &mut Document,
    shading_id: lopdf::ObjectId,
    resources: &Dictionary,
    mapper: &F,
    limits: &Limits,
) -> Result<(), &'static str>
where
    F: Fn(ColorSpace, &[f32]) -> Option<MappedColor>,
{
    let shading = doc
        .get_object(shading_id)
        .ok()
        .and_then(|o| o.as_dict().ok())
        .ok_or("missing shading")?;
    if !matches!(
        shading.get(b"ShadingType").and_then(Object::as_i64),
        Ok(2 | 3)
    ) {
        return Err("unsupported shading type");
    }
    let color = shading
        .get(b"ColorSpace")
        .map_err(|_| "missing shading colour space")?;
    let source = if let Ok(name) = color.as_name() {
        space(doc, resources, name, limits.max_nesting_depth)
    } else {
        let array = resolve(doc, color, limits.max_nesting_depth).and_then(|o| o.as_array().ok());
        array.and_then(|a| {
            if a.first()?.as_name().ok()? != b"ICCBased" {
                return None;
            }
            let profile = resolve(doc, a.get(1)?, limits.max_nesting_depth)?
                .as_stream()
                .ok()?;
            match profile.dict.get(b"N").and_then(Object::as_i64).ok()? {
                1 => Some(ColorSpace::Gray),
                3 => Some(ColorSpace::Rgb),
                4 => Some(ColorSpace::Cmyk),
                _ => None,
            }
        })
    }
    .ok_or("unsupported shading colour space")?;
    let domain =
        numbers_or_default(shading, b"Domain", &[0.0, 1.0]).ok_or("invalid shading domain")?;
    if domain.len() != 2
        || !domain[0].is_finite()
        || !domain[1].is_finite()
        || domain[0] >= domain[1]
    {
        return Err("invalid shading domain");
    }
    let function = shading
        .get(b"Function")
        .map_err(|_| "missing shading function")?;
    let mut data = Vec::new();
    let mut target = None;
    for i in 0..256 {
        let x = domain[0] + (domain[1] - domain[0]) * (i as f32 / 255.0);
        let components = sample_function(doc, function, x, 0, limits.max_nesting_depth)
            .ok_or("unsupported shading function")?;
        if components.len() != source.count() {
            return Err("invalid shading components");
        }
        let mapped = mapper(source, &components).ok_or("mapper declined shading")?;
        if mapped.components.len() != mapped.space.count()
            || mapped.components.iter().any(|v| !v.is_finite())
        {
            return Err("invalid mapped shading");
        }
        if target.is_some_and(|t| t != mapped.space) {
            return Err("inconsistent mapped shading space");
        }
        target = Some(mapped.space);
        data.extend(
            mapped
                .components
                .into_iter()
                .map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8),
        );
    }
    let target = target.ok_or("empty shading")?;
    let mut range = Vec::new();
    for _ in 0..target.count() {
        range.push(Object::Integer(0));
        range.push(Object::Integer(1));
    }
    let function = lopdf::Stream::new(
        lopdf::dictionary! {"FunctionType"=>0, "Domain"=>vec![Object::Real(domain[0]),Object::Real(domain[1])], "Range"=>range, "Size"=>vec![Object::Integer(256)], "BitsPerSample"=>8},
        data,
    );
    let id = doc.add_object(function);
    let shading = doc
        .get_object_mut(shading_id)
        .map_err(|_| "missing shading")?
        .as_dict_mut()
        .map_err(|_| "missing shading")?;
    shading.set("ColorSpace", Object::Name(target.name().to_vec()));
    shading.set("Function", id);
    Ok(())
}

pub(crate) fn selected_pages(
    pdf: &crate::PdfDoc,
    pages: &ariad_core::tools::page_range::PageRange,
    limits: &ariad_core::limits::Limits,
    ctx: &crate::ToolCtx,
) -> Result<Vec<u32>, crate::PdfError> {
    let count = crate::color::page_ids(pdf.inner(), limits, ctx)?.len();
    let count = u32::try_from(count).map_err(|_| crate::PdfError::Malformed)?;
    pages
        .expand(
            count,
            ariad_core::tools::page_range::DuplicatePolicy::Reject,
        )
        .map_err(|e| crate::PdfError::InvalidParams(e.to_string()))
}
