/*
 * fontconfig/fc-fontations/pcf.rs
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

//! Parser for X11 Portable Compiled Format (PCF) bitmap fonts.

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

const PCF_PROPERTIES: u32 = 1 << 0;
const PCF_ACCELERATORS: u32 = 1 << 1;
const PCF_BDF_ENCODINGS: u32 = 1 << 5;
const PCF_BDF_ACCELERATORS: u32 = 1 << 8;

const PCF_BYTE_MASK: u32 = 1 << 2;

/// Checks if bytes begin with the PCF header magic (`\x01fcp`).
pub fn is_pcf(data: &[u8]) -> bool {
    data.starts_with(b"\x01fcp")
}

#[derive(Debug, Clone)]
enum PropVal<'a> {
    Str(&'a str),
    Int(i32),
}

#[derive(Debug, Clone, Copy)]
struct TableEntry {
    type_: u32,
    _format: u32,
    size: usize,
    offset: usize,
}

fn read_u16(data: &[u8], big_endian: bool) -> u16 {
    if big_endian {
        u16::from_be_bytes(data[..2].try_into().unwrap())
    } else {
        u16::from_le_bytes(data[..2].try_into().unwrap())
    }
}

fn read_u32(data: &[u8], big_endian: bool) -> u32 {
    if big_endian {
        u32::from_be_bytes(data[..4].try_into().unwrap())
    } else {
        u32::from_le_bytes(data[..4].try_into().unwrap())
    }
}

fn read_i32(data: &[u8], big_endian: bool) -> i32 {
    if big_endian {
        i32::from_be_bytes(data[..4].try_into().unwrap())
    } else {
        i32::from_le_bytes(data[..4].try_into().unwrap())
    }
}

fn read_cstring(data: &[u8], offset: usize) -> Option<&str> {
    if offset >= data.len() {
        return None;
    }
    let s = &data[offset..];
    let len = s.iter().position(|&b| b == 0).unwrap_or(s.len());
    std::str::from_utf8(&s[..len]).ok()
}

fn parse_toc(data: &[u8]) -> Option<Vec<TableEntry>> {
    if data.len() < 8 || !is_pcf(data) {
        return None;
    }
    let table_count = u32::from_le_bytes(data[4..8].try_into().unwrap()) as usize;
    if table_count > 32 || data.len() < 8 + table_count * 16 {
        return None;
    }
    let mut tables = Vec::with_capacity(table_count);
    for i in 0..table_count {
        let entry_bytes = &data[8 + i * 16..8 + (i + 1) * 16];
        let type_ = u32::from_le_bytes(entry_bytes[0..4].try_into().unwrap());
        let format = u32::from_le_bytes(entry_bytes[4..8].try_into().unwrap());
        let size = u32::from_le_bytes(entry_bytes[8..12].try_into().unwrap()) as usize;
        let offset = u32::from_le_bytes(entry_bytes[12..16].try_into().unwrap()) as usize;
        if offset >= data.len() {
            return None;
        }
        let available_size = data.len() - offset;
        let actual_size = size.min(available_size);
        tables.push(TableEntry {
            type_,
            _format: format,
            size: actual_size,
            offset,
        });
    }
    Some(tables)
}

fn parse_properties<'a>(data: &'a [u8], entry: &TableEntry) -> Option<HashMap<&'a str, PropVal<'a>>> {
    let slice = &data[entry.offset..entry.offset + entry.size];
    if slice.len() < 8 {
        return None;
    }
    let format = u32::from_le_bytes(slice[0..4].try_into().unwrap());
    let big_endian = (format & PCF_BYTE_MASK) != 0;
    let nprops = read_u32(&slice[4..8], big_endian) as usize;

    let props_len = nprops.checked_mul(9)?;
    if slice.len() < 8 + props_len {
        return None;
    }
    let pad = (4 - (props_len % 4)) % 4;
    let strings_hdr_pos = 8 + props_len + pad;
    if slice.len() < strings_hdr_pos + 4 {
        return None;
    }
    let strings_len = read_u32(&slice[strings_hdr_pos..strings_hdr_pos + 4], big_endian) as usize;
    let strings_pos = strings_hdr_pos + 4;
    if slice.len() < strings_pos + strings_len {
        return None;
    }
    let strings_data = &slice[strings_pos..strings_pos + strings_len];

    let mut map = HashMap::with_capacity(nprops);
    for i in 0..nprops {
        let p = 8 + i * 9;
        let name_offset = read_u32(&slice[p..p + 4], big_endian) as usize;
        let is_string = slice[p + 4] != 0;
        let value = read_i32(&slice[p + 5..p + 9], big_endian);

        if let Some(name) = read_cstring(strings_data, name_offset) {
            let val = if is_string {
                if let Some(s) = read_cstring(strings_data, value as usize) {
                    PropVal::Str(s)
                } else {
                    PropVal::Str("")
                }
            } else {
                PropVal::Int(value)
            };
            map.insert(name, val);
        }
    }
    Some(map)
}

fn parse_encodings(
    data: &[u8],
    entry: &TableEntry,
    props: &HashMap<&str, PropVal>,
) -> Option<FcCharSetWrapper> {
    let slice = &data[entry.offset..entry.offset + entry.size];
    if slice.len() < 14 {
        return None;
    }
    let format = u32::from_le_bytes(slice[0..4].try_into().unwrap());
    let big_endian = (format & PCF_BYTE_MASK) != 0;
    let min_char_or_byte2 = read_u16(&slice[4..6], big_endian);
    let max_char_or_byte2 = read_u16(&slice[6..8], big_endian);
    let min_byte1 = read_u16(&slice[8..10], big_endian);
    let max_byte1 = read_u16(&slice[10..12], big_endian);

    if max_char_or_byte2 < min_char_or_byte2 || max_byte1 < min_byte1 {
        return None;
    }
    let cols = (max_char_or_byte2 - min_char_or_byte2 + 1) as usize;
    let rows = (max_byte1 - min_byte1 + 1) as usize;
    let n_entries = cols.checked_mul(rows)?;
    if slice.len() < 14 + n_entries * 2 {
        return None;
    }

    let charset_reg = match props.get("CHARSET_REGISTRY") {
        Some(PropVal::Str(s)) => *s,
        _ => "",
    };
    let charset_enc = match props.get("CHARSET_ENCODING") {
        Some(PropVal::Str(s)) => *s,
        _ => "",
    };

    let is_unicode = charset_reg.eq_ignore_ascii_case("ISO10646")
        || charset_reg.eq_ignore_ascii_case("UNICODE");
    let is_latin1 = charset_reg.eq_ignore_ascii_case("ISO8859") && charset_enc == "1";

    if !is_unicode && !is_latin1 {
        // FreeType does not synthesize a Unicode charmap for non-Unicode PCF fonts,
        // and fontconfig skips PCF fonts with 0 Unicode characters.
        return None;
    }

    let mut cs = FcCharSetWrapper::new()?;
    let mut num_added = 0;
    let indices_slice = &slice[14..14 + n_entries * 2];
    for (i, chunk) in indices_slice.chunks_exact(2).enumerate() {
        let glyph_idx = read_u16(chunk, big_endian);
        if glyph_idx == 0xffff {
            continue;
        }
        let b1 = min_byte1 as u32 + (i / cols) as u32;
        let b2 = min_char_or_byte2 as u32 + (i % cols) as u32;
        let code = (b1 << 8) | b2;
        if is_unicode {
            if char::from_u32(code).is_some() && cs.add_char(code).is_ok() {
                num_added += 1;
            }
        } else if is_latin1 && code <= 0xff && cs.add_char(code).is_ok() {
            num_added += 1;
        }
    }

    if num_added == 0 {
        return None;
    }
    Some(cs)
}

fn derive_style(props: &HashMap<&str, PropVal>) -> String {
    let mut strings: [Option<String>; 4] = [None, None, None, None];

    if let Some(PropVal::Str(add_style)) = props.get("ADD_STYLE_NAME") {
        let s = add_style.trim();
        if !s.is_empty() && !s.starts_with('N') && !s.starts_with('n') {
            strings[0] = Some(s.replace(' ', "-"));
        }
    }

    if let Some(PropVal::Str(weight)) = props.get("WEIGHT_NAME") {
        let w = weight.trim();
        if w.starts_with('B') || w.starts_with('b') {
            strings[1] = Some("Bold".to_string());
        }
    }

    if let Some(PropVal::Str(slant)) = props.get("SLANT") {
        let s = slant.trim();
        if s.starts_with('O') || s.starts_with('o') {
            strings[2] = Some("Oblique".to_string());
        } else if s.starts_with('I') || s.starts_with('i') {
            strings[2] = Some("Italic".to_string());
        }
    }

    if let Some(PropVal::Str(setwidth)) = props.get("SETWIDTH_NAME") {
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

fn derive_width(props: &HashMap<&str, PropVal>, style: &str) -> f64 {
    if let Some(PropVal::Int(setwidth)) = props.get("RELATIVE_SETWIDTH") {
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
    if let Some(PropVal::Str(setwidth_name)) = props.get("SETWIDTH_NAME") {
        if let Some(w) = contains_width(setwidth_name) {
            return w;
        }
    }
    contains_width(style).unwrap_or(FC_WIDTH_NORMAL as f64)
}

fn derive_pixelsize(props: &HashMap<&str, PropVal>) -> Option<f64> {
    if let Some(PropVal::Int(px)) = props.get("PIXEL_SIZE") {
        return Some(*px as f64);
    }
    if let (Some(PropVal::Int(pt)), Some(PropVal::Int(y_res))) =
        (props.get("POINT_SIZE"), props.get("RESOLUTION_Y"))
    {
        return Some((*pt as f64) * (*y_res as f64) / 720.0);
    }
    None
}

fn derive_spacing(
    props: &HashMap<&str, PropVal>,
    data: &[u8],
    accel_entry: Option<&TableEntry>,
) -> i32 {
    if let Some(PropVal::Str(sp)) = props.get("SPACING") {
        match sp.trim() {
            "c" | "C" => return FC_SPACING_CHARCELL as i32,
            "m" | "M" => return FC_SPACING_MONO as i32,
            "p" | "P" => return FC_SPACING_PROPORTIONAL as i32,
            _ => {}
        }
    }
    if let Some(entry) = accel_entry {
        let slice = &data[entry.offset..entry.offset + entry.size];
        if slice.len() >= 8 {
            let constant_width = slice[7];
            if constant_width != 0 {
                return FC_SPACING_MONO as i32;
            }
        }
    }
    FC_SPACING_PROPORTIONAL as i32
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

/// Builds Fontconfig patterns for a PCF bitmap font.
pub fn build_patterns_for_pcf(data: &[u8], font_file: &Path) -> Option<Vec<*mut FcPattern>> {
    let tables = parse_toc(data)?;

    let props_entry = tables.iter().find(|t| t.type_ == PCF_PROPERTIES)?;
    let enc_entry = tables.iter().find(|t| t.type_ == PCF_BDF_ENCODINGS)?;
    let accel_entry = tables
        .iter()
        .find(|t| t.type_ == PCF_BDF_ACCELERATORS || t.type_ == PCF_ACCELERATORS);

    let props = parse_properties(data, props_entry)?;
    let charset = parse_encodings(data, enc_entry, &props)?;

    let raw_family = match props.get("FAMILY_NAME") {
        Some(PropVal::Str(s)) if !s.trim().is_empty() => s.trim(),
        _ => font_file
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown"),
    };

    let foundry = match props.get("FOUNDRY") {
        Some(PropVal::Str(s)) if !s.trim().is_empty() => s.trim(),
        _ => "unknown",
    };

    // FreeType's default behavior (with PCF_CONFIG_OPTION_LONG_FAMILY_NAMES):
    // Prepend FOUNDRY to FAMILY_NAME if FAMILY_NAME does not already begin with FOUNDRY.
    let family = if foundry != "unknown"
        && !raw_family
            .to_ascii_lowercase()
            .starts_with(&foundry.to_ascii_lowercase())
    {
        format!("{foundry} {raw_family}")
    } else {
        raw_family.to_string()
    };

    let style = derive_style(&props);
    let slant_str = match props.get("SLANT") {
        Some(PropVal::Str(s)) => Some(*s),
        _ => None,
    };
    let weight_str = match props.get("WEIGHT_NAME") {
        Some(PropVal::Str(s)) => Some(*s),
        _ => None,
    };

    let slant = derive_slant(slant_str, &style);
    let weight = derive_weight(weight_str, &style);
    let width = derive_width(&props, &style);
    let pixelsize = derive_pixelsize(&props).unwrap_or(0.0);
    let spacing = derive_spacing(&props, data, accel_entry);
    let decorative = contains_decorative(&style);

    let mut pattern = FcPatternBuilder::new();

    let path_str = font_file.to_string_lossy().into_owned();
    if let Ok(cpath) = CString::new(path_str) {
        pattern.append_element(PatternElement::new(FC_FILE_OBJECT as i32, cpath.into()));
    }

    pattern.append_element(PatternElement::new(FC_INDEX_OBJECT as i32, 0.into()));
    pattern.append_element(PatternElement::new(
        FC_FONTFORMAT_OBJECT as i32,
        CString::new("PCF").unwrap().into(),
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
        .map(|wrapper| vec![wrapper.into_raw() as *mut FcPattern])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_pcf() {
        assert!(is_pcf(b"\x01fcp\x00\x00\x00\x00"));
        assert!(!is_pcf(b"OTTO"));
        assert!(!is_pcf(b"STARTFONT"));
    }

    #[test]
    fn test_parse_4x6_pcf() {
        let p = Path::new("../test/4x6.pcf");
        if !p.exists() {
            return;
        }
        let bytes = std::fs::read(p).unwrap();
        assert!(is_pcf(&bytes));
        let patterns = build_patterns_for_pcf(&bytes, p);
        assert!(patterns.is_some(), "Failed to parse 4x6.pcf");
    }
}
