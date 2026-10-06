/*
 * fontconfig/fc-fontations/bdf.rs
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

//! Parser for Adobe Glyph Bitmap Distribution Format (BDF) fonts.

use std::collections::HashMap;
use std::ffi::CString;
use std::path::Path;

use fcint_bindings::{
    FcLangSetFromCharSet, FC_ANTIALIAS_OBJECT, FC_CHARSET_OBJECT, FC_COLOR_OBJECT,
    FC_DECORATIVE_OBJECT, FC_FAMILYLANG_OBJECT, FC_FAMILY_OBJECT, FC_FILE_OBJECT,
    FC_FONTFORMAT_OBJECT, FC_FONTVERSION_OBJECT, FC_FONT_HAS_HINT_OBJECT, FC_FOUNDRY_OBJECT,
    FC_FULLNAMELANG_OBJECT, FC_FULLNAME_OBJECT, FC_INDEX_OBJECT, FC_LANG_OBJECT,
    FC_NAMED_INSTANCE_OBJECT, FC_ORDER_OBJECT, FC_OUTLINE_OBJECT, FC_PIXEL_SIZE_OBJECT,
    FC_POSTSCRIPT_NAME_OBJECT, FC_SCALABLE_OBJECT, FC_SLANT_OBJECT, FC_SPACING_OBJECT,
    FC_STYLELANG_OBJECT, FC_STYLE_OBJECT, FC_SYMBOL_OBJECT, FC_VARIABLE_OBJECT,
    FC_WEIGHT_OBJECT, FC_WIDTH_OBJECT,
};

use fontconfig_bindings::{
    FcPattern, FC_SLANT_ITALIC, FC_SLANT_OBLIQUE, FC_SLANT_ROMAN, FC_SPACING_CHARCELL,
    FC_SPACING_MONO, FC_SPACING_PROPORTIONAL, FC_WEIGHT_MEDIUM, FC_WIDTH_CONDENSED,
    FC_WIDTH_EXPANDED, FC_WIDTH_EXTRACONDENSED, FC_WIDTH_EXTRAEXPANDED, FC_WIDTH_NORMAL,
    FC_WIDTH_SEMICONDENSED, FC_WIDTH_SEMIEXPANDED, FC_WIDTH_ULTRACONDENSED, FC_WIDTH_ULTRAEXPANDED,
};

use crate::pattern_bindings::{
    fc_wrapper::{FcCharSetWrapper, FcLangSetWrapper},
    FcPatternBuilder, PatternElement,
};
use crate::style_consts::{
    contains_decorative, contains_slant, contains_weight, contains_width, is_weight,
};

/// Checks if bytes begin with a BDF header (`STARTFONT`).
pub fn is_bdf(data: &[u8]) -> bool {
    data.starts_with(b"STARTFONT ") || data.starts_with(b"STARTFONT\t")
}

#[derive(Debug, Clone)]
enum BdfProp<'a> {
    Str(&'a str),
    Int(i32),
}

fn parse_quoted_or_raw(s: &str) -> &str {
    let trimmed = s.trim();
    if trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2 {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    }
}

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

fn derive_style(props: &HashMap<&str, BdfProp>) -> String {
    let mut strings: [Option<String>; 4] = [None, None, None, None];

    if let Some(BdfProp::Str(add_style)) = props.get("ADD_STYLE_NAME") {
        let s = add_style.trim();
        if !s.is_empty() && !s.starts_with('N') && !s.starts_with('n') {
            strings[0] = Some(s.replace(' ', "-"));
        }
    }

    if let Some(BdfProp::Str(weight)) = props.get("WEIGHT_NAME") {
        let w = weight.trim();
        if w.starts_with('B') || w.starts_with('b') {
            strings[1] = Some("Bold".to_string());
        }
    }

    if let Some(BdfProp::Str(slant)) = props.get("SLANT") {
        let s = slant.trim();
        if s.starts_with('O') || s.starts_with('o') {
            strings[2] = Some("Oblique".to_string());
        } else if s.starts_with('I') || s.starts_with('i') {
            strings[2] = Some("Italic".to_string());
        }
    }

    if let Some(BdfProp::Str(setwidth)) = props.get("SETWIDTH_NAME") {
        let s = setwidth.trim();
        if !s.is_empty() && !s.starts_with('N') && !s.starts_with('n') {
            strings[3] = Some(s.replace(' ', "-"));
        }
    }

    let non_empty: Vec<&str> = strings.iter().filter_map(|s| s.as_deref()).collect();
    if non_empty.is_empty() {
        "Regular".to_string()
    } else {
        non_empty.join(" ")
    }
}

fn derive_slant(slant_str: Option<&str>, style: &str) -> i32 {
    if let Some(s) = slant_str {
        match s.trim() {
            "I" | "i" => return FC_SLANT_ITALIC as i32,
            "O" | "o" => return FC_SLANT_OBLIQUE as i32,
            "R" | "r" => return FC_SLANT_ROMAN as i32,
            _ => {}
        }
    }
    contains_slant(style).unwrap_or(FC_SLANT_ROMAN as i32)
}

fn derive_weight(weight_name: Option<&str>, style: &str) -> f64 {
    contains_weight(style)
        .or_else(|| weight_name.and_then(is_weight))
        .unwrap_or(FC_WEIGHT_MEDIUM as f64)
}

fn derive_width(props: &HashMap<&str, BdfProp>, style: &str) -> f64 {
    if let Some(BdfProp::Int(setwidth)) = props.get("RELATIVE_SETWIDTH") {
        let val = (setwidth + 5) / 10;
        match val {
            1 => return FC_WIDTH_ULTRACONDENSED as f64,
            2 => return FC_WIDTH_EXTRACONDENSED as f64,
            3 => return FC_WIDTH_CONDENSED as f64,
            4 => return FC_WIDTH_SEMICONDENSED as f64,
            5 => return FC_WIDTH_NORMAL as f64,
            6 => return FC_WIDTH_SEMIEXPANDED as f64,
            7 => return FC_WIDTH_EXPANDED as f64,
            8 => return FC_WIDTH_EXTRAEXPANDED as f64,
            9 => return FC_WIDTH_ULTRAEXPANDED as f64,
            _ => {}
        }
    }
    if let Some(BdfProp::Str(setwidth_name)) = props.get("SETWIDTH_NAME") {
        if let Some(w) = contains_width(setwidth_name) {
            return w;
        }
    }
    contains_width(style).unwrap_or(FC_WIDTH_NORMAL as f64)
}

fn derive_spacing(props: &HashMap<&str, BdfProp>) -> i32 {
    if let Some(BdfProp::Str(sp)) = props.get("SPACING") {
        match sp.trim() {
            "c" | "C" => return FC_SPACING_CHARCELL as i32,
            "m" | "M" => return FC_SPACING_MONO as i32,
            "p" | "P" => return FC_SPACING_PROPORTIONAL as i32,
            _ => {}
        }
    }
    FC_SPACING_PROPORTIONAL as i32
}

/// Builds a Fontconfig pattern for a BDF font.
pub fn build_pattern_for_bdf(data: &[u8], font_file: &Path) -> Option<*mut FcPattern> {
    if !is_bdf(data) {
        return None;
    }

    let text = std::str::from_utf8(data).ok()?;

    let mut props: HashMap<&str, BdfProp> = HashMap::new();
    let mut in_properties = false;
    let mut size_pixel: Option<f64> = None;
    let mut font_name: Option<&str> = None;

    let mut char_codes: Vec<u32> = Vec::new();

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if in_properties {
            if line == "ENDPROPERTIES" {
                in_properties = false;
                continue;
            }
            if let Some(space_idx) = line.find(' ') {
                let key = &line[..space_idx];
                let val_str = line[space_idx + 1..].trim();
                let prop_val = if val_str.starts_with('"') {
                    BdfProp::Str(parse_quoted_or_raw(val_str))
                } else if let Ok(n) = val_str.parse::<i32>() {
                    BdfProp::Int(n)
                } else {
                    BdfProp::Str(val_str)
                };
                props.insert(key, prop_val);
            }
            continue;
        }

        if line.starts_with("STARTPROPERTIES") {
            in_properties = true;
            continue;
        }

        if let Some(rest) = line.strip_prefix("FONT ") {
            font_name = Some(rest.trim());
        } else if let Some(rest) = line.strip_prefix("SIZE ") {
            let mut parts = rest.split_whitespace();
            if let (Some(pt_str), Some(_xres_str), Some(yres_str)) =
                (parts.next(), parts.next(), parts.next())
            {
                if let (Ok(pt), Ok(yres)) = (pt_str.parse::<f64>(), yres_str.parse::<f64>()) {
                    size_pixel = Some(pt * yres / 72.0);
                }
            }
        } else if let Some(rest) = line.strip_prefix("ENCODING ") {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            let code_int = if parts.len() >= 2 && parts[0] == "-1" {
                parts[1].parse::<i64>().ok()
            } else if let Some(p) = parts.first() {
                p.parse::<i64>().ok()
            } else {
                None
            };
            if let Some(c) = code_int {
                if c >= 0 && c <= 0x10ffff {
                    char_codes.push(c as u32);
                }
            }
        }
    }

    let charset_reg = match props.get("CHARSET_REGISTRY") {
        Some(BdfProp::Str(s)) => *s,
        _ => "",
    };
    let charset_enc = match props.get("CHARSET_ENCODING") {
        Some(BdfProp::Str(s)) => *s,
        _ => "",
    };

    let is_unicode = charset_reg.eq_ignore_ascii_case("ISO10646")
        || charset_reg.eq_ignore_ascii_case("UNICODE");
    let is_latin1 = charset_reg.eq_ignore_ascii_case("ISO8859") && charset_enc == "1";

    if !is_unicode && !is_latin1 {
        // FreeType does not synthesize a Unicode charmap for non-Unicode BDF fonts,
        // and fontconfig skips BDF fonts without Unicode encodings.
        return None;
    }

    let mut charset = FcCharSetWrapper::new()?;
    let mut num_added = 0;
    for code in char_codes {
        if is_unicode {
            if char::from_u32(code).is_some() && charset.add_char(code).is_ok() {
                num_added += 1;
            }
        } else if is_latin1 && code <= 0xff && charset.add_char(code).is_ok() {
            num_added += 1;
        }
    }

    if num_added == 0 {
        return None;
    }

    // In BDF, FreeType uses FAMILY_NAME property directly without prepending FOUNDRY.
    let family = match props.get("FAMILY_NAME") {
        Some(BdfProp::Str(s)) if !s.trim().is_empty() => s.trim().to_string(),
        _ => font_name
            .map(|s| s.to_string())
            .or_else(|| font_file.file_stem().and_then(|s| s.to_str()).map(|s| s.to_string()))
            .unwrap_or_else(|| "unknown".to_string()),
    };

    let foundry = match props.get("FOUNDRY") {
        Some(BdfProp::Str(s)) if !s.trim().is_empty() => s.trim(),
        _ => "unknown",
    };

    let style = derive_style(&props);
    let slant_str = match props.get("SLANT") {
        Some(BdfProp::Str(s)) => Some(*s),
        _ => None,
    };
    let weight_str = match props.get("WEIGHT_NAME") {
        Some(BdfProp::Str(s)) => Some(*s),
        _ => None,
    };

    let slant = derive_slant(slant_str, &style);
    let weight = derive_weight(weight_str, &style);
    let width = derive_width(&props, &style);
    let pixelsize = match props.get("PIXEL_SIZE") {
        Some(BdfProp::Int(px)) => *px as f64,
        _ => size_pixel.unwrap_or(0.0),
    };
    let spacing = derive_spacing(&props);
    let decorative = contains_decorative(&style);

    let mut pattern = FcPatternBuilder::new();

    let path_str = font_file.to_string_lossy().into_owned();
    if let Ok(cpath) = CString::new(path_str) {
        pattern.append_element(PatternElement::new(FC_FILE_OBJECT as i32, cpath.into()));
    }

    pattern.append_element(PatternElement::new(FC_INDEX_OBJECT as i32, 0.into()));
    pattern.append_element(PatternElement::new(
        FC_FONTFORMAT_OBJECT as i32,
        CString::new("BDF").unwrap().into(),
    ));
    pattern.append_element(PatternElement::new(FC_FONTVERSION_OBJECT as i32, 0.into()));
    pattern.append_element(PatternElement::new(FC_OUTLINE_OBJECT as i32, false.into()));
    pattern.append_element(PatternElement::new(FC_COLOR_OBJECT as i32, false.into()));
    pattern.append_element(PatternElement::new(FC_SCALABLE_OBJECT as i32, false.into()));
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
    pattern.append_element(PatternElement::new(
        FC_ANTIALIAS_OBJECT as i32,
        false.into(),
    ));

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

    if let Ok(c_style) = CString::new(style.as_str()) {
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

    let psname = sanitize_ps_name(&family);
    if let Ok(c_psname) = CString::new(psname) {
        pattern.append_element(PatternElement::new(
            FC_POSTSCRIPT_NAME_OBJECT as i32,
            c_psname.into(),
        ));
    }

    pattern.append_element(PatternElement::new(FC_SLANT_OBJECT as i32, slant.into()));
    pattern.append_element(PatternElement::new(FC_WEIGHT_OBJECT as i32, weight.into()));
    pattern.append_element(PatternElement::new(FC_WIDTH_OBJECT as i32, width.into()));

    if pixelsize > 0.0 {
        pattern.append_element(PatternElement::new(
            FC_PIXEL_SIZE_OBJECT as i32,
            pixelsize.into(),
        ));
    }

    if spacing != (FC_SPACING_PROPORTIONAL as i32) {
        pattern.append_element(PatternElement::new(
            FC_SPACING_OBJECT as i32,
            spacing.into(),
        ));
    }

    if let Ok(c_foundry) = CString::new(foundry) {
        pattern.append_element(PatternElement::new(
            FC_FOUNDRY_OBJECT as i32,
            c_foundry.into(),
        ));
    }

    pattern.append_element(PatternElement::new(
        FC_DECORATIVE_OBJECT as i32,
        decorative.into(),
    ));

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
    fn test_is_bdf() {
        assert!(is_bdf(b"STARTFONT 2.1\n"));
        assert!(is_bdf(b"STARTFONT\t2.2\n"));
        assert!(!is_bdf(b"\x01fcp"));
        assert!(!is_bdf(b"OTTO"));
    }
}
