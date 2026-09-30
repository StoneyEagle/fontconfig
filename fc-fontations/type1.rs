/*
 * fontconfig/fc-fontations/type1.rs
 *
 * Copyright 2026 Google LLC.
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

use std::ffi::CString;
use std::path::Path;

use fcint_bindings::{
    FcLangSetFromCharSet, FC_CHARSET_OBJECT, FC_COLOR_OBJECT, FC_DECORATIVE_OBJECT,
    FC_FAMILYLANG_OBJECT, FC_FAMILY_OBJECT, FC_FILE_OBJECT, FC_FONTFORMAT_OBJECT,
    FC_FONTVERSION_OBJECT, FC_FONT_HAS_HINT_OBJECT, FC_FOUNDRY_OBJECT, FC_FULLNAMELANG_OBJECT,
    FC_FULLNAME_OBJECT, FC_INDEX_OBJECT, FC_LANG_OBJECT, FC_NAMED_INSTANCE_OBJECT, FC_ORDER_OBJECT,
    FC_OUTLINE_OBJECT, FC_POSTSCRIPT_NAME_OBJECT, FC_SCALABLE_OBJECT, FC_SLANT_OBJECT,
    FC_SPACING_OBJECT, FC_STYLELANG_OBJECT, FC_STYLE_OBJECT, FC_SYMBOL_OBJECT, FC_VARIABLE_OBJECT,
    FC_WEIGHT_OBJECT, FC_WIDTH_OBJECT,
};

use fontconfig_bindings::{
    FcPattern, FC_SLANT_ITALIC, FC_SLANT_ROMAN, FC_SPACING_MONO, FC_WEIGHT_MEDIUM, FC_WIDTH_NORMAL,
};

use crate::foundries::notice_to_foundry;
use crate::postscript::{postscript_flag, postscript_string};
use crate::style_consts::{
    contains_decorative, contains_slant, contains_weight, contains_width as width, is_weight,
};

use crate::pattern_bindings::{
    fc_wrapper::{FcCharSetWrapper, FcLangSetWrapper},
    FcPatternBuilder, PatternElement,
};

use read_fonts::ps::type1::Type1Font;

/// Whether these bytes are a Type 1 font rather than an SFNT.
///
/// `%!` opens the plain PFA form; `0x80 0x01` opens a PFB segment header.
pub fn is_type1(data: &[u8]) -> bool {
    data.starts_with(b"%!") || data.starts_with(&[0x80, 0x01])
}

fn notice_foundry(data: &[u8]) -> Option<&'static str> {
    let header = &data[..data.len().min(64 * 1024)];
    notice_to_foundry(postscript_string(header, b"/Notice")?)
}

fn weight(font: &Type1Font, style: &str) -> Option<f64> {
    font.weight()
        .and_then(is_weight)
        .or_else(|| contains_weight(style))
}

fn slant(font: &Type1Font, style: &str) -> Option<i32> {
    contains_slant(style).or_else(|| (font.italic_angle() != 0).then_some(FC_SLANT_ITALIC as i32))
}

/// Strips the `family` prefix from `full`, ignoring spaces and hyphens.
///
/// Returns the remaining suffix of `full` starting immediately after the
/// matched family characters, or `None` if `full` does not begin with `family`.
fn strip_family_prefix<'a>(full: &'a str, family: &str) -> Option<&'a str> {
    let is_delim = |c: char| c == ' ' || c == '-';
    let mut fam_chars = family.chars().filter(|&c| !is_delim(c)).peekable();

    if fam_chars.peek().is_none() {
        return Some(full);
    }

    for (idx, ch) in full.char_indices() {
        if is_delim(ch) {
            continue;
        }
        if fam_chars.next() != Some(ch) {
            return None;
        }
        if fam_chars.peek().is_none() {
            return Some(&full[idx + ch.len_utf8()..]);
        }
    }

    None
}

/// Extracts the style name from `full_name` by stripping the `family_name` prefix,
/// ignoring spaces and hyphens, mirroring FreeType's `t1objs.c`.
fn extract_style<'a>(full: &'a str, family: &str) -> Option<&'a str> {
    let is_delim = |c: char| c == ' ' || c == '-';
    strip_family_prefix(full, family)
        .map(|suffix| suffix.trim_matches(is_delim))
        .map(|style| if style.is_empty() { "Regular" } else { style })
}

/// Derives the family name, mirroring FreeType's `t1objs.c` and Fontconfig's `fcfreetype.c`.
fn family(font: &Type1Font, font_file: &Path) -> Option<String> {
    let font_name = font
        .family_name()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| font.name().filter(|s| !s.trim().is_empty()));

    if let Some(name) = font_name {
        Some(name.to_string())
    } else {
        font_file
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
    }
}

/// Derives the style name, mirroring FreeType's `t1objs.c`.
///
/// If both `/FamilyName` and `/FullName` are present, attempts to extract the style
/// by stripping the family prefix from the full name. Otherwise, falls back to `/Weight`.
fn style<'a>(font: &'a Type1Font) -> Option<&'a str> {
    // FreeType's t1objs.c first tries to extract the style from full_name and family_name,
    // falling back to the /Weight dictionary entry if extraction fails or names are absent.
    if let Some(family) = font.family_name() {
        if let Some(full) = font.full_name() {
            if let Some(style) = extract_style(full, family) {
                return Some(style);
            }
        }
    }

    font.weight().filter(|s| !s.trim().is_empty())
}

/// Sanitizes a string for use as a PostScript literal name by replacing
/// delimiter and whitespace characters with hyphens and truncating to 255 chars,
/// mirroring Fontconfig's `fcfreetype.c`.
fn sanitize_ps_name(s: &str) -> String {
    s.chars()
        .take(255)
        .map(|c| match c {
            '\x04' | '(' | ')' | '/' | '<' | '>' | '[' | ']' | '{' | '}' | '\t' | '\x0c' | '\r'
            | '\n' | ' ' => '-',
            _ => c,
        })
        .collect()
}

/// Derives the PostScript name for the font, mirroring Fontconfig's `fcfreetype.c`.
///
/// Uses `/FontName` if present, or converts the family name by replacing
/// PostScript delimiter and whitespace characters with hyphens.
fn ps_name(font: &Type1Font, family: &str) -> Option<String> {
    if let Some(name) = font.name().filter(|s| !s.is_empty()) {
        return Some(name.to_string());
    }

    if family.is_empty() {
        return None;
    }

    Some(sanitize_ps_name(family))
}

/// Extracts the character set from the Type 1 font.
///
/// In `read-fonts`, character maps derived from PostScript glyph names set bit 31
/// (`0x8000_0000`, matching FreeType's `VARIANT_BIT` in `psnames/psmodule.c`) on
/// variant glyphs (such as `A.alt` or `foo.swash`). Masking out `0x8000_0000`
/// recovers the base Unicode codepoint.
fn charset(font: &Type1Font) -> Option<FcCharSetWrapper> {
    let mut charset = FcCharSetWrapper::new()?;
    for (code, _gid) in font.unicode_charmap().iter() {
        // read-fonts marks variant glyphs with bit 31 (0x8000_0000, VARIANT_BIT)
        // following FreeType's psnames/psmodule.c. Clear it to get the base codepoint.
        let base = code & !0x8000_0000;
        if char::from_u32(base).is_some() {
            let _ = charset.add_char(base);
        }
    }
    for (_gid, name) in font.glyph_names() {
        if let Some(cp) = crate::zapf::codepoint(name) {
            let _ = charset.add_char(cp);
        }
    }
    Some(charset)
}

/// Builds a Fontconfig pattern for a Type 1 font file.
pub fn build_pattern_for_type1(bytes: &[u8], font_file: &Path) -> Option<*mut FcPattern> {
    let font = Type1Font::new(bytes).ok()?;
    let mut pattern = FcPatternBuilder::new();

    let path_str = font_file.to_string_lossy().into_owned();
    if let Ok(cpath) = CString::new(path_str) {
        pattern.append_element(PatternElement::new(FC_FILE_OBJECT as i32, cpath.into()));
    }

    pattern.append_element(PatternElement::new(FC_INDEX_OBJECT as i32, 0.into()));
    pattern.append_element(PatternElement::new(
        FC_FONTFORMAT_OBJECT as i32,
        CString::new("Type 1").unwrap().into(),
    ));
    pattern.append_element(PatternElement::new(FC_FONTVERSION_OBJECT as i32, 0.into()));
    pattern.append_element(PatternElement::new(FC_OUTLINE_OBJECT as i32, true.into()));
    pattern.append_element(PatternElement::new(FC_COLOR_OBJECT as i32, false.into()));
    pattern.append_element(PatternElement::new(FC_SCALABLE_OBJECT as i32, true.into()));
    pattern.append_element(PatternElement::new(
        FC_FONT_HAS_HINT_OBJECT as i32,
        false.into(),
    ));
    pattern.append_element(PatternElement::new(FC_ORDER_OBJECT as i32, 0.into()));
    pattern.append_element(PatternElement::new(FC_SYMBOL_OBJECT as i32, false.into()));
    pattern.append_element(PatternElement::new(FC_VARIABLE_OBJECT as i32, false.into()));
    pattern.append_element(PatternElement::new(
        FC_NAMED_INSTANCE_OBJECT as i32,
        false.into(),
    ));

    let family = family(&font, font_file).unwrap_or_else(|| "unknown".to_string());
    let style = style(&font).unwrap_or("Regular");

    if let Ok(c_family) = CString::new(family.as_str()) {
        pattern.append_element(PatternElement::new(
            FC_FAMILY_OBJECT as i32,
            c_family.into(),
        ));
        pattern.append_element(PatternElement::new(
            FC_FAMILYLANG_OBJECT as i32,
            CString::new("en").unwrap().into(),
        ));

        crate::names::append_generic_families(&mut pattern);
    }

    if let Ok(c_style) = CString::new(style) {
        pattern.append_element(PatternElement::new(FC_STYLE_OBJECT as i32, c_style.into()));
        pattern.append_element(PatternElement::new(
            FC_STYLELANG_OBJECT as i32,
            CString::new("en").unwrap().into(),
        ));
    }

    let fullname = format!("{} {}", family.trim(), style.trim());
    if let Ok(c_fullname) = CString::new(fullname) {
        pattern.append_element(PatternElement::new(
            FC_FULLNAME_OBJECT as i32,
            c_fullname.into(),
        ));
        pattern.append_element(PatternElement::new(
            FC_FULLNAMELANG_OBJECT as i32,
            CString::new("en").unwrap().into(),
        ));
    }

    if let Some(ps_name) = ps_name(&font, &family) {
        if let Ok(c_ps_name) = CString::new(ps_name) {
            pattern.append_element(PatternElement::new(
                FC_POSTSCRIPT_NAME_OBJECT as i32,
                c_ps_name.into(),
            ));
        }
    }

    let slant = slant(&font, style).unwrap_or(FC_SLANT_ROMAN as i32);
    pattern.append_element(PatternElement::new(FC_SLANT_OBJECT as i32, slant.into()));

    let weight = weight(&font, style).unwrap_or(FC_WEIGHT_MEDIUM as f64);
    pattern.append_element(PatternElement::new(FC_WEIGHT_OBJECT as i32, weight.into()));

    let width = width(style).unwrap_or(FC_WIDTH_NORMAL as f64);
    pattern.append_element(PatternElement::new(FC_WIDTH_OBJECT as i32, width.into()));

    let foundry = notice_foundry(bytes).unwrap_or("unknown");
    if let Ok(c_foundry) = CString::new(foundry) {
        pattern.append_element(PatternElement::new(
            FC_FOUNDRY_OBJECT as i32,
            c_foundry.into(),
        ));
    }

    let decorative = contains_decorative(style);
    pattern.append_element(PatternElement::new(
        FC_DECORATIVE_OBJECT as i32,
        decorative.into(),
    ));

    // Note: read-fonts parses `IsFixedPitch` (PascalCase), but Section 5.2 of
    // the Adobe Type 1 specification defines `/isFixedPitch` (camelCase).
    // In practice, standard fonts (e.g. Source Code Pro) use `/isFixedPitch`.
    // We check both `font.is_fixed_pitch()` and fallback to `postscript_flag(bytes, b"/isFixedPitch")`.
    if font.is_fixed_pitch() || postscript_flag(bytes, b"/isFixedPitch") {
        pattern.append_element(PatternElement::new(
            FC_SPACING_OBJECT as i32,
            (FC_SPACING_MONO as i32).into(),
        ));
    }

    let charset = charset(&font)?;
    let langset = unsafe {
        FcLangSetWrapper::from_raw(FcLangSetFromCharSet(charset.as_ptr(), std::ptr::null()))
    };

    pattern.append_element(PatternElement::new(
        FC_CHARSET_OBJECT as i32,
        charset.into(),
    ));

    if !langset.is_null() {
        pattern.append_element(PatternElement::new(FC_LANG_OBJECT as i32, langset.into()));
    }

    pattern
        .create_fc_pattern()
        .map(|wrapper| wrapper.into_raw() as *mut FcPattern)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_type1() {
        assert!(is_type1(b"%!PS-AdobeFont-1.0"));
        assert!(is_type1(&[0x80, 0x01, 0x00, 0x00]));
        assert!(!is_type1(b"\x00\x01\x00\x00"));
        assert!(!is_type1(b"OTTO"));
        assert!(!is_type1(b"wOF2"));
        assert!(!is_type1(b""));
    }

    #[test]
    fn test_notice_foundry() {
        let urw_notice =
            b"/Notice ((URW)++,Copyright 2014 by (URW)++ Design & Development) readonly def";
        assert_eq!(notice_foundry(urw_notice), Some("urw"));

        let bitstream_notice = b"/Notice (Copyright 1990 Bitstream Inc.) def";
        assert_eq!(notice_foundry(bitstream_notice), Some("bitstream"));

        let adobe_notice = b"/Notice (Copyright 1989 Adobe Systems Incorporated.) def";
        assert_eq!(notice_foundry(adobe_notice), Some("adobe"));

        let unknown_notice = b"/Notice (Unknown foundry 2026) def";
        assert_eq!(notice_foundry(unknown_notice), None);
    }

    #[test]
    fn test_weights() {
        assert_eq!(is_weight("Regular"), Some(80.0));
        assert_eq!(is_weight("Bold"), Some(200.0));
        assert_eq!(is_weight("Normal"), Some(80.0));
        assert_eq!(is_weight("Roman"), None);

        assert_eq!(contains_weight("Bold Italic"), Some(200.0));
        assert_eq!(contains_weight("Regular"), Some(80.0));
        assert_eq!(contains_weight("Oblique"), None);
    }

    #[test]
    fn test_slants() {
        assert_eq!(contains_slant("Italic"), Some(100));
        assert_eq!(contains_slant("Bold Italic"), Some(100));
        assert_eq!(contains_slant("Oblique"), Some(110));
        assert_eq!(contains_slant("Regular"), None);
    }

    #[test]
    fn test_widths() {
        assert_eq!(width("Condensed"), Some(75.0));
        assert_eq!(width("Bold Condensed"), Some(75.0));
        assert_eq!(width("UltraExpanded"), Some(200.0));
        assert_eq!(width("Regular"), None);
    }

    #[test]
    fn test_extract_style() {
        assert_eq!(
            extract_style("Nimbus Roman Bold", "Nimbus Roman"),
            Some("Bold")
        );
        assert_eq!(
            extract_style("Nimbus Roman-Bold", "Nimbus Roman"),
            Some("Bold")
        );
        assert_eq!(
            extract_style("Nimbus Roman", "Nimbus Roman"),
            Some("Regular")
        );
        assert_eq!(
            extract_style("Nimbus-Roman", "Nimbus Roman"),
            Some("Regular")
        );
        assert_eq!(
            extract_style("NimbusRoman-Bold", "Nimbus Roman"),
            Some("Bold")
        );
        assert_eq!(
            extract_style("Nimbus Roman Bold Italic", "Nimbus Roman"),
            Some("Bold Italic")
        );
        assert_eq!(
            extract_style("Nimbus Roman - ", "Nimbus Roman"),
            Some("Regular")
        );
        assert_eq!(extract_style("Nimbus Roman", "Nimbus Roman Bold"), None);
        assert_eq!(extract_style("Times New Roman", "Helvetica"), None);
        assert_eq!(
            extract_style("Pazo Math Italic", "PazoMath"),
            Some("Italic")
        );
        assert_eq!(
            extract_style("Tlwg Typewriter Oblique", "TlwgTypewriter"),
            Some("Oblique")
        );
        assert_eq!(
            extract_style("Tlwg Typewriter", "TlwgTypewriter"),
            Some("Regular")
        );
    }

    #[test]
    fn test_ps_name_exclusive_chars() {
        assert_eq!(sanitize_ps_name("Nimbus Roman"), "Nimbus-Roman");
        assert_eq!(sanitize_ps_name("Font(Name)"), "Font-Name-");
        assert_eq!(sanitize_ps_name("Foo/Bar<Baz>[Qux]"), "Foo-Bar-Baz--Qux-");
        assert_eq!(
            sanitize_ps_name("Line1\r\nLine2\tLine3\x0cEnd"),
            "Line1--Line2-Line3-End"
        );
    }
}
