/*
 * fontconfig/fc-fontations/attributes.rs
 *
 * Copyright 2025 Google LLC.
 *
 * Permission to use, copy, modify, distribute, and sell this software and its
 * documentation for any purpose is hereby granted without fee, provided that
 * the above copyright notice appear in all copies and that both that
 * copyright notice and this permission notice appear in supporting
 * documentation, and that the name of the author(s) not be used in
 * advertising or publicity pertaining to distribution of the software without
 * specific, written prior permission.  The authors make no
 * representations about the suitability of this software for any purpose.  It
 * is provided "as is" without express or implied warranty.
 *
 * THE AUTHOR(S) DISCLAIMS ALL WARRANTIES WITH REGARD TO THIS SOFTWARE,
 * INCLUDING ALL IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS, IN NO
 * EVENT SHALL THE AUTHOR(S) BE LIABLE FOR ANY SPECIAL, INDIRECT OR
 * CONSEQUENTIAL DAMAGES OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS OF USE,
 * DATA OR PROFITS, WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER
 * TORTIOUS ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR
 * PERFORMANCE OF THIS SOFTWARE.
 */

use fontconfig_bindings::{
    FcWeightFromOpenTypeDouble, FC_SLANT_ITALIC, FC_SLANT_OBLIQUE, FC_SLANT_ROMAN, FC_SPACING_DUAL,
    FC_SPACING_MONO, FC_WEIGHT_BLACK, FC_WEIGHT_BOLD, FC_WEIGHT_EXTRABOLD, FC_WEIGHT_EXTRALIGHT,
    FC_WEIGHT_LIGHT, FC_WEIGHT_MEDIUM, FC_WEIGHT_NORMAL, FC_WEIGHT_SEMIBOLD, FC_WEIGHT_THIN,
    FC_WIDTH_CONDENSED, FC_WIDTH_EXPANDED, FC_WIDTH_EXTRACONDENSED, FC_WIDTH_EXTRAEXPANDED,
    FC_WIDTH_NORMAL, FC_WIDTH_SEMICONDENSED, FC_WIDTH_SEMIEXPANDED, FC_WIDTH_ULTRACONDENSED,
    FC_WIDTH_ULTRAEXPANDED,
};

use fcint_bindings::{
    FC_DECORATIVE_OBJECT, FC_INDEX_OBJECT, FC_NAMED_INSTANCE_OBJECT, FC_SIZE_OBJECT,
    FC_SLANT_OBJECT, FC_SPACING_OBJECT, FC_STYLE_OBJECT, FC_VARIABLE_OBJECT, FC_WEIGHT_OBJECT,
    FC_WIDTH_OBJECT,
};

use crate::{
    pattern_bindings::{
        fc_wrapper::FcRangeWrapper, FcPatternBuilder, PatternElement, PatternValue,
    },
    InstanceMode,
};
use read_fonts::TableProvider;
use skrifa::{
    attribute::{Attributes, Stretch, Style, Weight},
    instance::Location,
    metrics::GlyphMetrics,
    prelude::{LocationRef, Size},
    AxisCollection, FontRef, MetadataProvider, NamedInstance, Tag,
};

fn fc_weight(skrifa_weight: Weight) -> f64 {
    (match skrifa_weight {
        Weight::THIN => FC_WEIGHT_THIN,
        Weight::EXTRA_LIGHT => FC_WEIGHT_EXTRALIGHT,
        Weight::LIGHT => FC_WEIGHT_LIGHT,
        Weight::NORMAL => FC_WEIGHT_NORMAL,
        Weight::MEDIUM => FC_WEIGHT_MEDIUM,
        Weight::SEMI_BOLD => FC_WEIGHT_SEMIBOLD,
        Weight::BOLD => FC_WEIGHT_BOLD,
        Weight::EXTRA_BOLD => FC_WEIGHT_EXTRABOLD,
        Weight::BLACK => FC_WEIGHT_BLACK,
        // See fcfreetype.c: When weight is not available, set to medium.
        // This would mean a font did not have a parseable OS/2 table or
        // a weight value could not be retrieved from it.
        _ => FC_WEIGHT_MEDIUM,
    }) as f64
}

fn fc_slant(skrifa_style: Style) -> u32 {
    match skrifa_style {
        Style::Italic => FC_SLANT_ITALIC,
        Style::Oblique(_) => FC_SLANT_OBLIQUE,
        _ => FC_SLANT_ROMAN,
    }
}

fn fc_width(skrifa_stretch: Stretch) -> f64 {
    (match skrifa_stretch {
        Stretch::ULTRA_CONDENSED => FC_WIDTH_ULTRACONDENSED,
        Stretch::EXTRA_CONDENSED => FC_WIDTH_EXTRACONDENSED,
        Stretch::CONDENSED => FC_WIDTH_CONDENSED,
        Stretch::SEMI_CONDENSED => FC_WIDTH_SEMICONDENSED,
        Stretch::NORMAL => FC_WIDTH_NORMAL,
        Stretch::SEMI_EXPANDED => FC_WIDTH_SEMIEXPANDED,
        Stretch::EXPANDED => FC_WIDTH_EXPANDED,
        Stretch::EXTRA_EXPANDED => FC_WIDTH_EXTRAEXPANDED,
        Stretch::ULTRA_EXPANDED => FC_WIDTH_ULTRAEXPANDED,
        _ => FC_WIDTH_NORMAL,
    } as f64)
}

fn fc_weight_from_os2(font_ref: &FontRef) -> Option<f64> {
    let us_weight = font_ref.os2().ok()?.us_weight_class() as f64;
    unsafe {
        let result = FcWeightFromOpenTypeDouble(us_weight);
        if result == -1.0 {
            None
        } else {
            Some(result)
        }
    }
}

fn fc_width_from_os2(font_ref: &FontRef) -> Option<f64> {
    let us_width = font_ref.os2().ok()?.us_width_class();
    let converted = match us_width {
        1 => FC_WIDTH_ULTRACONDENSED,
        2 => FC_WIDTH_EXTRACONDENSED,
        3 => FC_WIDTH_CONDENSED,
        4 => FC_WIDTH_SEMICONDENSED,
        5 => FC_WIDTH_NORMAL,
        6 => FC_WIDTH_SEMIEXPANDED,
        7 => FC_WIDTH_EXPANDED,
        8 => FC_WIDTH_EXTRAEXPANDED,
        9 => FC_WIDTH_ULTRAEXPANDED,
        _ => FC_WIDTH_NORMAL,
    };
    Some(converted as f64)
}

fn fc_size_from_os2(font_ref: &FontRef) -> Option<(f64, f64)> {
    font_ref.os2().ok().and_then(|os2| {
        Some((
            os2.us_lower_optical_point_size()? as f64 / 20.0,
            os2.us_upper_optical_point_size()? as f64 / 20.0,
        ))
    })
}

struct AttributesToPattern<'a> {
    weight_from_os2: Option<f64>,
    width_from_os2: Option<f64>,
    attributes: Attributes,
    axes: AxisCollection<'a>,
    named_instance: Option<NamedInstance<'a>>,
    font_ref: FontRef<'a>,
}

impl<'a> AttributesToPattern<'a> {
    fn new(font: &'a FontRef, instance_mode: &InstanceMode) -> Self {
        let named_instance = match instance_mode {
            InstanceMode::Named(index) => font.named_instances().get(*index as usize),
            _ => None,
        };

        Self {
            weight_from_os2: fc_weight_from_os2(font),
            width_from_os2: fc_width_from_os2(font),
            attributes: Attributes::new(font),
            axes: font.axes(),
            named_instance,
            font_ref: font.clone(),
        }
    }

    fn user_coord_for_tag(&self, tag: Tag) -> Option<f64> {
        let mut axis_coords = self
            .axes
            .iter()
            .map(|axis| axis.tag())
            .zip(self.named_instance.clone()?.user_coords());
        Some(axis_coords.find(|item| item.0 == tag)?.1 as f64)
    }

    fn flags_weight(&self) -> PatternElement {
        PatternElement::new(
            FC_WEIGHT_OBJECT as i32,
            fc_weight(self.attributes.weight).into(),
        )
    }

    fn os2_weight(&self) -> Option<PatternElement> {
        self.weight_from_os2
            .map(|weight| PatternElement::new(FC_WEIGHT_OBJECT as i32, weight.into()))
    }

    fn flags_width(&self) -> PatternElement {
        PatternElement::new(
            FC_WIDTH_OBJECT as i32,
            fc_width(self.attributes.stretch).into(),
        )
    }

    fn os2_width(&self) -> Option<PatternElement> {
        self.width_from_os2
            .map(|width| PatternElement::new(FC_WIDTH_OBJECT as i32, width.into()))
    }

    fn static_slant(&self) -> PatternElement {
        PatternElement::new(
            FC_SLANT_OBJECT as i32,
            (fc_slant(self.attributes.style) as i32).into(),
        )
    }

    fn instance_weight(&self) -> Option<PatternElement> {
        let named_instance_weight = self.user_coord_for_tag(Tag::new(b"wght"))?;
        unsafe {
            Some(PatternElement::new(
                FC_WEIGHT_OBJECT as i32,
                FcWeightFromOpenTypeDouble(named_instance_weight).into(),
            ))
        }
    }

    fn instance_width(&self) -> Option<PatternElement> {
        let named_instance_weight = self.user_coord_for_tag(Tag::new(b"wdth"))?;

        Some(PatternElement::new(
            FC_WIDTH_OBJECT as i32,
            named_instance_weight.into(),
        ))
    }

    fn instance_slant(&self) -> Option<PatternElement> {
        let named_instance_slant = self.user_coord_for_tag(Tag::new(b"slnt"))?;
        if named_instance_slant < 0.0 {
            Some(PatternElement::new(
                FC_SLANT_OBJECT as i32,
                (FC_SLANT_ITALIC as i32).into(),
            ))
        } else {
            Some(PatternElement::new(
                FC_SLANT_OBJECT as i32,
                (FC_SLANT_ROMAN as i32).into(),
            ))
        }
    }

    fn instance_size(&self) -> Option<PatternElement> {
        let named_instance_size = self.user_coord_for_tag(Tag::new(b"opsz"))?;

        Some(PatternElement::new(
            FC_SIZE_OBJECT as i32,
            named_instance_size.into(),
        ))
    }

    fn default_axis_size(&self) -> Option<PatternElement> {
        self.axes.get_by_tag(Tag::new(b"opsz")).map(|opsz_axis| {
            PatternElement::new(
                FC_SIZE_OBJECT as i32,
                (opsz_axis.default_value() as f64).into(),
            )
        })
    }

    fn os2_size(&self) -> Option<PatternElement> {
        fc_size_from_os2(&self.font_ref).and_then(|(lower, higher)| {
            if lower != higher {
                let range = FcRangeWrapper::new(lower, higher)?;
                Some(PatternElement::new(FC_SIZE_OBJECT as i32, range.into()))
            } else {
                Some(PatternElement::new(FC_SIZE_OBJECT as i32, lower.into()))
            }
        })
    }

    fn variable_weight(&self) -> Option<PatternElement> {
        let weight_axis = self.axes.get_by_tag(Tag::new(b"wght"))?;
        unsafe {
            Some(PatternElement::new(
                FC_WEIGHT_OBJECT as i32,
                FcRangeWrapper::new(
                    FcWeightFromOpenTypeDouble(weight_axis.min_value() as f64),
                    FcWeightFromOpenTypeDouble(weight_axis.max_value() as f64),
                )?
                .into(),
            ))
        }
    }

    fn variable_width(&self) -> Option<PatternElement> {
        let width_axis = self.axes.get_by_tag(Tag::new(b"wdth"))?;
        Some(PatternElement::new(
            FC_WIDTH_OBJECT as i32,
            FcRangeWrapper::new(width_axis.min_value() as f64, width_axis.max_value() as f64)?
                .into(),
        ))
    }

    fn variable_opsz(&self) -> Option<PatternElement> {
        let opsz_axis = self.axes.get_by_tag(Tag::new(b"opsz"))?;
        Some(PatternElement::new(
            FC_SIZE_OBJECT as i32,
            FcRangeWrapper::new(opsz_axis.min_value() as f64, opsz_axis.max_value() as f64)?.into(),
        ))
    }

    // Determine FontConfig spacing property.
    //
    // This mirrors FcSpacingClassify() in src/fcfreetype.c -- the two must stay
    // in sync so the FreeType and Fontations backends agree (enforced by
    // test_fontations_ft_query.py).  It runs in two stages:
    //   1. reproduce the legacy rule (single width -> MONO; two widths in a 2:1
    //      ratio -> DUAL), so nothing that already had a spacing is reclassified;
    //   2. only when the legacy rule yields proportional, recover a grid font --
    //      one whose nonzero advances are integer multiples k*W of a base cell W
    //      -- but only if the font monospaces its ASCII letters.  That gate is
    //      what keeps uniform full-width scripts, emoji and symbol fonts (grid
    //      conformant but not monospaced Latin) from being promoted.
    //
    // Takes (codepoint, advance) pairs.  Returns None for proportional fonts (no
    // FC_SPACING element is emitted).
    fn spacing_from_advances(advances: impl Iterator<Item = (u32, f32)>) -> Option<i32> {
        const MAX_CLUSTERS: usize = 64; // real grid fonts have only a few distinct advances
        const GRID_PERCENT: i64 = 98; // min % of advances that must sit on the cell grid
        const SIGNIFICANT: i64 = 20; // a base-cell candidate holds >= total/20 (5%)
        const MIN_ASCII: i64 = 20; // min ASCII letters/digits needed to judge Latin spacing

        let approximately_equal =
            |a: f32, b: f32| (a - b).abs() * 33.0 <= a.abs().max(b.abs());

        // Cluster nonzero advances by 3% tolerance, and track whether the ASCII
        // letters/digits share a single advance.
        let mut value: Vec<f32> = Vec::new();
        let mut count: Vec<i64> = Vec::new();
        let mut total: i64 = 0;
        let mut overflow = false;
        let mut ascii_width = 0.0f32;
        let mut ascii_count: i64 = 0;
        let mut ascii_varies = false;

        'collect: for (codepoint, advance) in advances {
            if advance <= 0.0 {
                continue;
            }
            if (0x30..=0x39).contains(&codepoint)
                || (0x41..=0x5A).contains(&codepoint)
                || (0x61..=0x7A).contains(&codepoint)
            {
                if ascii_count == 0 {
                    ascii_width = advance;
                } else if !approximately_equal(advance, ascii_width) {
                    ascii_varies = true;
                }
                ascii_count += 1;
            }
            if overflow {
                continue;
            }
            for (i, v) in value.iter().enumerate() {
                if approximately_equal(advance, *v) {
                    count[i] += 1;
                    total += 1;
                    continue 'collect;
                }
            }
            if value.len() >= MAX_CLUSTERS {
                // Far more distinct widths than any cell-grid font: proportional.
                overflow = true;
                continue;
            }
            value.push(advance);
            count.push(1);
            total += 1;
        }

        // Stage 1 -- legacy classification, preserved verbatim.
        if !overflow {
            // No advances, or every glyph the same width: MONO.
            if total <= 0 || value.len() <= 1 {
                return Some(FC_SPACING_MONO as i32);
            }
            // Exactly two widths in a 2:1 ratio: DUAL.
            if value.len() == 2 {
                let lo = value[0].min(value[1]);
                let hi = value[0].max(value[1]);
                if approximately_equal(lo * 2.0, hi) {
                    return Some(FC_SPACING_DUAL as i32);
                }
            }
        }

        // Stage 2 -- recover grid fonts that monospace their ASCII.
        let ascii_monospaced = ascii_count >= MIN_ASCII && !ascii_varies;
        if overflow || !ascii_monospaced {
            return None;
        }

        let on_grid_for = |w: f32| -> i64 {
            let mut on = 0;
            for (i, v) in value.iter().enumerate() {
                let mult = (*v / w).round();
                if mult >= 1.0 && approximately_equal(*v, mult * w) {
                    on += count[i];
                }
            }
            on
        };

        // Try every significant cluster's width as the base cell W, and keep
        // whichever puts the most advances on an integer grid.  Only real
        // clusters are candidates: synthesising finer ones (e.g. W/2) would let
        // the scattered advances of a proportional font land on a dense grid by
        // coincidence.
        let mut best_on_grid: i64 = 0;
        let mut best_w: f32 = 0.0;
        for i in 0..value.len() {
            if count[i] * SIGNIFICANT < total {
                continue; // ignore minor clusters as base-cell candidates
            }
            let w = value[i];
            if w <= 0.0 {
                continue;
            }
            let on = on_grid_for(w);
            if on > best_on_grid {
                best_on_grid = on;
                best_w = w;
            }
        }

        if best_w <= 0.0 || best_on_grid * 100 < total * GRID_PERCENT {
            return None;
        }

        // On a grid: single-width is mono, dual-width (half/full cell, e.g. CJK)
        // is dual.  Decide from the most populated cluster.
        let mut dom = 0;
        for i in 1..count.len() {
            if count[i] > count[dom] {
                dom = i;
            }
        }
        if approximately_equal(value[dom], 2.0 * best_w) {
            Some(FC_SPACING_DUAL as i32)
        } else {
            Some(FC_SPACING_MONO as i32)
        }
    }

    fn spacing(&self) -> Option<PatternElement> {
        let mut location = Location::default();
        if let Some(instance) = &self.named_instance {
            location = instance.location().clone();
        };

        let glyph_metrics = GlyphMetrics::new(
            &self.font_ref,
            Size::new(16.0),
            LocationRef::from(&location),
        );

        let advances = self
            .font_ref
            .charmap()
            .mappings()
            .filter_map(|(codepoint, gid)| {
                glyph_metrics
                    .advance_width(gid)
                    .and_then(|adv| if adv > 0.0 { Some((codepoint, adv)) } else { None })
            });

        Self::spacing_from_advances(advances)
            .map(|spacing| PatternElement::new(FC_SPACING_OBJECT as i32, spacing.into()))
    }
}

#[derive(Default)]
struct AttributesFromStyleString {
    weight: Option<PatternElement>,
    width: Option<PatternElement>,
    slant: Option<PatternElement>,
    decorative: Option<PatternElement>,
}

impl AttributesFromStyleString {
    fn new(pattern: &FcPatternBuilder) -> Self {
        let style_string = pattern
            .into_iter()
            .find(|element| element.object_id == FC_STYLE_OBJECT as i32)
            .and_then(|element| match &element.value {
                PatternValue::String(style) => Some(style),
                _ => None,
            });

        if let Some(style) = style_string {
            let style_str = style.to_str().unwrap_or_default();
            Self {
                weight: crate::style_consts::contains_weight(style_str)
                    .map(|w| PatternElement::new(FC_WEIGHT_OBJECT as i32, w.into())),
                width: crate::style_consts::contains_width(style_str)
                    .map(|w| PatternElement::new(FC_WIDTH_OBJECT as i32, w.into())),
                slant: crate::style_consts::contains_slant(style_str)
                    .map(|s| PatternElement::new(FC_SLANT_OBJECT as i32, s.into())),
                decorative: Some(PatternElement::new(
                    FC_DECORATIVE_OBJECT as i32,
                    crate::style_consts::contains_decorative(style_str).into(),
                )),
            }
        } else {
            Self {
                weight: None,
                width: None,
                slant: None,
                decorative: Some(PatternElement::new(
                    FC_DECORATIVE_OBJECT as i32,
                    false.into(),
                )),
            }
        }
    }
}

/// Appends style pattern elements such as weight, width, slant, decorative to the pattern.
/// Requires a textual style element to be already added to the pattern, so it's good
/// to run this after names have been added. This is because this method performs certain
/// string matches on the font name to determine style attributes.
pub fn append_style_elements(
    font: &FontRef,
    instance_mode: InstanceMode,
    ttc_index: Option<i32>,
    pattern: &mut FcPatternBuilder,
) {
    // TODO: fcfreetype.c seems to prefer parsing information from the WWS name table entry,
    // but falls back to flags if those are not found. So far, I haven't identified test fonts
    // for which the WWS code path would trigger.

    let attributes_text = AttributesFromStyleString::new(pattern);

    let skrifa_attributes = AttributesToPattern::new(font, &instance_mode);

    if let Some(spacing) = skrifa_attributes.spacing() {
        pattern.append_element(spacing);
    }

    match instance_mode {
        InstanceMode::Default => {
            let pattern_weight = skrifa_attributes
                .os2_weight()
                .or(attributes_text.weight)
                .unwrap_or(skrifa_attributes.flags_weight());
            pattern.append_element(pattern_weight);

            let width = skrifa_attributes
                .os2_width()
                .or(attributes_text.width)
                .unwrap_or(skrifa_attributes.flags_width());
            pattern.append_element(width);

            pattern.append_element(
                attributes_text
                    .slant
                    .unwrap_or(skrifa_attributes.static_slant()),
            );

            if let Some(element) = attributes_text.decorative {
                pattern.append_element(element)
            }

            pattern.append_element(PatternElement::new(FC_VARIABLE_OBJECT as i32, false.into()));
            pattern.append_element(PatternElement::new(
                FC_INDEX_OBJECT as i32,
                ttc_index.unwrap_or_default().into(),
            ));

            if let Some(size) = skrifa_attributes
                .default_axis_size()
                .or(skrifa_attributes.os2_size())
            {
                pattern.append_element(size);
            }

            pattern.append_element(PatternElement::new(
                FC_NAMED_INSTANCE_OBJECT as i32,
                false.into(),
            ));
        }
        InstanceMode::Variable => {
            let weight = skrifa_attributes
                .variable_weight()
                .or(skrifa_attributes.os2_weight())
                .or(attributes_text.weight)
                .unwrap_or(skrifa_attributes.flags_weight());
            pattern.append_element(weight);

            let width = skrifa_attributes
                .variable_width()
                .or(skrifa_attributes.os2_width())
                .or(attributes_text.width)
                .unwrap_or(skrifa_attributes.flags_width());
            pattern.append_element(width);

            if let Some(element) = attributes_text.decorative {
                pattern.append_element(element)
            }

            if let Some(size) = skrifa_attributes.variable_opsz() {
                pattern.append_element(size);
            }

            pattern.append_element(PatternElement::new(FC_VARIABLE_OBJECT as i32, true.into()));

            // TODO: Check if this should have a zero ttc index if not part of a collection.
            pattern.append_element(PatternElement::new(
                FC_INDEX_OBJECT as i32,
                ttc_index.unwrap_or_default().into(),
            ));
            pattern.append_element(PatternElement::new(
                FC_NAMED_INSTANCE_OBJECT as i32,
                false.into(),
            ));
            pattern.append_element(skrifa_attributes.static_slant());
        }
        InstanceMode::Named(index) => {
            let weight = skrifa_attributes
                .instance_weight()
                .or(attributes_text.weight)
                .unwrap_or(skrifa_attributes.flags_weight());
            pattern.append_element(weight);

            let width = skrifa_attributes
                .instance_width()
                .or(attributes_text.width)
                .unwrap_or(skrifa_attributes.flags_width());
            pattern.append_element(width);

            pattern.append_element(
                skrifa_attributes
                    .instance_slant()
                    .or(attributes_text.slant)
                    .unwrap_or(skrifa_attributes.static_slant()),
            );

            if let Some(element) = attributes_text.decorative {
                pattern.append_element(element)
            }

            pattern.append_element(PatternElement::new(FC_VARIABLE_OBJECT as i32, false.into()));
            pattern.append_element(PatternElement::new(
                FC_INDEX_OBJECT as i32,
                (ttc_index.unwrap_or_default() + ((index + 1) << 16)).into(),
            ));
            if let Some(size_element) = skrifa_attributes
                .instance_size()
                .or(skrifa_attributes.default_axis_size())
            {
                pattern.append_element(size_element);
            };

            pattern.append_element(PatternElement::new(
                FC_NAMED_INSTANCE_OBJECT as i32,
                true.into(),
            ));
        }
    }
}

#[cfg(test)]
mod test {
    use crate::attributes::AttributesToPattern;
    use fontconfig_bindings::{FC_SPACING_DUAL, FC_SPACING_MONO};

    // ASCII letter coverage for a synthetic font.
    enum Ascii {
        None,       // no Latin letters (e.g. CJK / emoji / symbol fonts)
        Mono(f32),  // Latin letters present and all at this advance (monospaced)
        Varies,     // Latin letters present but proportional
    }

    // Build (codepoint, advance) glyphs from (advance, count) cluster pairs.
    // The cluster glyphs are given non-ASCII codepoints so only the explicit
    // Ascii spec drives the ASCII-monospace gate.
    fn glyphs(pairs: &[(f32, usize)], ascii: Ascii) -> Vec<(u32, f32)> {
        let mut v = Vec::new();
        let mut cp = 0x3000u32; // CJK area: never ASCII
        for &(adv, count) in pairs {
            for _ in 0..count {
                v.push((cp, adv));
                cp += 1;
            }
        }
        match ascii {
            Ascii::None => {}
            // 26 lowercase letters (>= MIN_ASCII) at one advance.
            Ascii::Mono(w) => v.extend((0x61..=0x7Au32).map(|c| (c, w))),
            // 26 lowercase letters at increasing advances.
            Ascii::Varies => {
                v.extend((0x61..=0x7Au32).enumerate().map(|(i, c)| (c, 500.0 + i as f32 * 50.0)))
            }
        }
        v
    }

    // The cases below mirror test/test-spacing.c (the FreeType-side unit test)
    // so both backends are checked against the same real-font corpus.
    fn assert_spacing(pairs: &[(f32, usize)], ascii: Ascii, expectation: Option<i32>) {
        assert_eq!(
            AttributesToPattern::spacing_from_advances(glyphs(pairs, ascii).into_iter()),
            expectation
        );
    }

    #[test]
    fn spacing_mono() {
        // Uniform width -> MONO via the legacy stage (ASCII irrelevant).
        assert_spacing(&[(600.0, 1000)], Ascii::None, Some(FC_SPACING_MONO as i32));
        // Grid font with a 2x em dash plus a stray off-grid glyph, monospaced
        // ASCII: recovered as MONO (mirrors Noto Sans Mono at 99.97%).  The old
        // "first three distinct advances" rule wrongly reported this proportional.
        assert_spacing(
            &[(563.0, 9994), (1126.0, 3), (800.0, 3)],
            Ascii::Mono(563.0),
            Some(FC_SPACING_MONO as i32),
        );
    }

    #[test]
    fn spacing_dual() {
        // Two widths in a 2:1 ratio -> DUAL via the legacy stage, preserved even
        // without Latin (mirrors Droid Sans Japanese).
        assert_spacing(
            &[(128.0, 63), (256.0, 6555)],
            Ascii::None,
            Some(FC_SPACING_DUAL as i32),
        );
    }

    #[test]
    fn spacing_ascii_gate() {
        // Same uniform full-width grid, classified only by the ASCII gate:
        // promoted to MONO with monospaced ASCII, proportional without it
        // (the latter mirrors Noto Serif Tangut / Hentaigana / emoji).
        assert_spacing(
            &[(1000.0, 9800), (2000.0, 150), (3000.0, 50)],
            Ascii::Mono(1000.0),
            Some(FC_SPACING_MONO as i32),
        );
        assert_spacing(
            &[(1000.0, 9800), (2000.0, 150), (3000.0, 50)],
            Ascii::None,
            None,
        );
    }

    #[test]
    fn spacing_proportional() {
        // Proportional by design -- stays proportional even if named "Mono"
        // (mirrors Latin Modern Mono Prop at 27% grid conformance).
        assert_spacing(
            &[(261.0, 270), (400.0, 250), (550.0, 240), (700.0, 240)],
            Ascii::Mono(261.0),
            None,
        );
        // CJK "mono": 73% on the 500/1000 grid, ~26% Hangul off-grid at 920.
        // Half-width Latin is monospaced, yet the grid threshold still rejects it.
        assert_spacing(
            &[(1000.0, 6000), (500.0, 1300), (920.0, 2700)],
            Ascii::Mono(500.0),
            None,
        );
        // One dominant width (94.9%) plus a scattered tail -- proportional
        // (mirrors AlgolRevived).  The tail sits on a 63 (W/2) grid, so a
        // half-width base-cell candidate would wrongly promote this to DUAL;
        // only whole-cluster candidates keep it proportional.
        assert_spacing(
            &[
                (126.0, 9190),
                (190.0, 100),
                (310.0, 90),
                (440.0, 85),
                (570.0, 80),
                (695.0, 75),
                (820.0, 61),
            ],
            Ascii::Mono(126.0),
            None,
        );
        // Grid-conformant but with proportional Latin -> rejected by the ASCII
        // gate (e.g. an ideographic font that includes proportional Latin).
        assert_spacing(
            &[(1000.0, 9800), (2000.0, 150), (3000.0, 50)],
            Ascii::Varies,
            None,
        );
    }
}
