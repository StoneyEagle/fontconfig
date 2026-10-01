/*
 * fontconfig/fc-fontations/winfnt.rs
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

//! Parser for Windows FNT and FON bitmap fonts.

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
    FcPattern, FC_SLANT_ITALIC, FC_SLANT_ROMAN, FC_SPACING_MONO, FC_WEIGHT_BOLD,
    FC_WEIGHT_MEDIUM, FC_WIDTH_NORMAL,
};

use crate::pattern_bindings::{
    fc_wrapper::{FcCharSetWrapper, FcLangSetWrapper},
    FcPatternBuilder, PatternElement,
};
use crate::style_consts::contains_decorative;

const RT_FONT: u16 = 0x8008;

fn read_u16_le(data: &[u8], offset: usize) -> Option<u16> {
    data.get(offset..offset + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
}

fn read_u32_le(data: &[u8], offset: usize) -> Option<u32> {
    data.get(offset..offset + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

#[derive(Debug)]
#[allow(dead_code)]
struct WinFntHeader<'a> {
    version: u16,
    file_size: u32,
    nominal_point_size: u16,
    vertical_resolution: u16,
    italic: bool,
    weight: u16,
    pixel_height: u16,
    face_name: &'a str,
}

impl<'a> WinFntHeader<'a> {
    fn parse(data: &'a [u8]) -> Option<Self> {
        if data.len() < 118 {
            return None;
        }

        let version = read_u16_le(data, 0)?;
        if version != 0x200 && version != 0x300 {
            return None;
        }

        let file_size = read_u32_le(data, 2)?;
        let min_size = if version == 0x300 { 148 } else { 118 };
        if (file_size as usize) < min_size {
            return None;
        }

        let file_type = read_u16_le(data, 66)?;
        if file_type & 1 != 0 {
            // Vector FNT fonts are not supported
            return None;
        }

        let nominal_point_size = read_u16_le(data, 68)?;
        let vertical_resolution = read_u16_le(data, 70)?;
        let italic = data[80] != 0;
        let weight = read_u16_le(data, 83)?;
        let pixel_height = read_u16_le(data, 88)?;
        if pixel_height == 0 {
            return None;
        }

        let face_name_offset = read_u32_le(data, 105)? as usize;
        if face_name_offset >= data.len() {
            return None;
        }

        let name_bytes = &data[face_name_offset..];
        let null_pos = name_bytes.iter().position(|&b| b == 0).unwrap_or(name_bytes.len());
        let face_name = std::str::from_utf8(&name_bytes[..null_pos]).ok()?.trim();

        Some(WinFntHeader {
            version,
            file_size,
            nominal_point_size,
            vertical_resolution,
            italic,
            weight,
            pixel_height,
            face_name,
        })
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

/// Extract FNT resources from a 16-bit NE (New Executable) file.
fn extract_ne_fnts<'a>(data: &'a [u8], ne_offset: usize) -> Vec<&'a [u8]> {
    let mut fnts = Vec::new();
    let rsrc_tab_rel = match read_u16_le(data, ne_offset + 0x24) {
        Some(off) => off as usize,
        None => return fnts,
    };
    let rsrc_offset = ne_offset + rsrc_tab_rel;
    let align_shift = match read_u16_le(data, rsrc_offset) {
        Some(s) if s <= 16 => s,
        _ => return fnts,
    };

    let mut pos = rsrc_offset + 2;
    while pos + 8 <= data.len() {
        let type_id = match read_u16_le(data, pos) {
            Some(0) | None => break,
            Some(t) => t,
        };
        let count = match read_u16_le(data, pos + 2) {
            Some(c) => c as usize,
            None => break,
        };
        pos += 8;

        if pos + count * 12 > data.len() {
            break;
        }

        for i in 0..count {
            let entry_pos = pos + i * 12;
            let rn_offset = match read_u16_le(data, entry_pos) {
                Some(off) => (off as usize) << align_shift,
                None => continue,
            };
            let rn_length = match read_u16_le(data, entry_pos + 2) {
                Some(len) => (len as usize) << align_shift,
                None => continue,
            };

            if type_id == RT_FONT && rn_offset < data.len() {
                let end = rn_offset.saturating_add(rn_length).min(data.len());
                fnts.push(&data[rn_offset..end]);
            }
        }
        pos += count * 12;
    }

    fnts
}

/// Extract FNT resources from a 32-bit PE (Portable Executable) file.
fn extract_pe_fnts<'a>(data: &'a [u8], pe_offset: usize) -> Vec<&'a [u8]> {
    let mut fnts = Vec::new();
    let num_sections = match read_u16_le(data, pe_offset + 6) {
        Some(n) => n as usize,
        None => return fnts,
    };
    let opt_hdr_size = match read_u16_le(data, pe_offset + 20) {
        Some(s) => s as usize,
        None => return fnts,
    };

    let opt_hdr_offset = pe_offset + 24;
    let opt_magic = match read_u16_le(data, opt_hdr_offset) {
        Some(m) => m,
        None => return fnts,
    };

    let rsrc_dir_entry_offset = if opt_magic == 0x10b {
        opt_hdr_offset + 96 + 16 // PE32: data directory entry 2 (Resource Table)
    } else if opt_magic == 0x20b {
        opt_hdr_offset + 112 + 16 // PE32+
    } else {
        return fnts;
    };

    let rsrc_va = match read_u32_le(data, rsrc_dir_entry_offset) {
        Some(va) if va != 0 => va as usize,
        _ => return fnts,
    };

    let section_headers_offset = opt_hdr_offset + opt_hdr_size;
    let mut rsrc_section: Option<(usize, usize, usize)> = None; // (va, raw_data_ptr, raw_data_size)

    for i in 0..num_sections {
        let sec_offset = section_headers_offset + i * 40;
        let va = match read_u32_le(data, sec_offset + 12) {
            Some(v) => v as usize,
            None => continue,
        };
        let raw_size = match read_u32_le(data, sec_offset + 16) {
            Some(s) => s as usize,
            None => continue,
        };
        let raw_ptr = match read_u32_le(data, sec_offset + 20) {
            Some(p) => p as usize,
            None => continue,
        };

        if rsrc_va >= va && rsrc_va < va + raw_size {
            rsrc_section = Some((va, raw_ptr, raw_size));
            break;
        }
    }

    let (sec_va, sec_raw_ptr, _) = match rsrc_section {
        Some(sec) => sec,
        None => return fnts,
    };

    let rsrc_root_offset = sec_raw_ptr + (rsrc_va - sec_va);

    // Parse resource tree:
    // Root level: Resource Types
    let num_named = read_u16_le(data, rsrc_root_offset + 12).unwrap_or(0) as usize;
    let num_id = read_u16_le(data, rsrc_root_offset + 14).unwrap_or(0) as usize;
    let total_types = num_named + num_id;

    for i in 0..total_types {
        let entry_offset = rsrc_root_offset + 16 + i * 8;
        let type_id = read_u32_le(data, entry_offset).unwrap_or(0);
        let type_dir_offset = read_u32_le(data, entry_offset + 4).unwrap_or(0);

        if (type_id == 8 || type_id == RT_FONT as u32) && (type_dir_offset & 0x80000000 != 0) {
            let name_dir_offset = rsrc_root_offset + (type_dir_offset & 0x7FFFFFFF) as usize;
            let n_named = read_u16_le(data, name_dir_offset + 12).unwrap_or(0) as usize;
            let n_id = read_u16_le(data, name_dir_offset + 14).unwrap_or(0) as usize;

            for j in 0..(n_named + n_id) {
                let name_entry_offset = name_dir_offset + 16 + j * 8;
                let lang_dir_offset = read_u32_le(data, name_entry_offset + 4).unwrap_or(0);

                if lang_dir_offset & 0x80000000 != 0 {
                    let lang_offset = rsrc_root_offset + (lang_dir_offset & 0x7FFFFFFF) as usize;
                    let l_named = read_u16_le(data, lang_offset + 12).unwrap_or(0) as usize;
                    let l_id = read_u16_le(data, lang_offset + 14).unwrap_or(0) as usize;

                    for k in 0..(l_named + l_id) {
                        let lang_entry_offset = lang_offset + 16 + k * 8;
                        let data_leaf_offset = read_u32_le(data, lang_entry_offset + 4).unwrap_or(0);
                        if data_leaf_offset & 0x80000000 == 0 {
                            let data_entry = rsrc_root_offset + data_leaf_offset as usize;
                            let data_rva = read_u32_le(data, data_entry).unwrap_or(0) as usize;
                            let data_size = read_u32_le(data, data_entry + 4).unwrap_or(0) as usize;

                            if data_rva >= sec_va {
                                let file_offset = sec_raw_ptr + (data_rva - sec_va);
                                if file_offset < data.len() {
                                    let end = file_offset.saturating_add(data_size).min(data.len());
                                    fnts.push(&data[file_offset..end]);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    fnts
}

/// Extract all FNT slices from `data` (either standalone FNT or NE/PE FON).
fn extract_fnts<'a>(data: &'a [u8]) -> Vec<&'a [u8]> {
    if data.len() < 118 {
        return Vec::new();
    }

    // Standalone FNT
    let version = u16::from_le_bytes([data[0], data[1]]);
    if (version == 0x200 || version == 0x300) && (data[66] & 1 == 0) {
        return vec![data];
    }

    // MZ Executable (NE or PE)
    if data.starts_with(b"MZ") && data.len() >= 0x40 {
        if let Some(e_lfanew) = read_u32_le(data, 0x3c).map(|v| v as usize) {
            if e_lfanew + 2 <= data.len() {
                if &data[e_lfanew..e_lfanew + 2] == b"NE" {
                    return extract_ne_fnts(data, e_lfanew);
                }
                if e_lfanew + 4 <= data.len() && &data[e_lfanew..e_lfanew + 4] == b"PE\0\0" {
                    return extract_pe_fnts(data, e_lfanew);
                }
            }
        }
    }

    Vec::new()
}

/// Checks if `data` is a Windows FNT or FON bitmap font.
pub fn is_winfnt(data: &[u8]) -> bool {
    !extract_fnts(data).is_empty()
}

fn build_pattern_for_face(
    fnt_data: &[u8],
    index: usize,
    font_file: &Path,
) -> Option<*mut FcPattern> {
    let header = WinFntHeader::parse(fnt_data)?;

    let is_bold = header.weight >= 800;
    let is_italic = header.italic;

    let style = match (is_bold, is_italic) {
        (true, true) => "Bold Italic",
        (true, false) => "Bold",
        (false, true) => "Italic",
        (false, false) => "Regular",
    };

    let weight = crate::style_consts::contains_weight(style).unwrap_or(if is_bold {
        FC_WEIGHT_BOLD as f64
    } else {
        FC_WEIGHT_MEDIUM as f64
    });

    let slant = if is_italic {
        FC_SLANT_ITALIC as i32
    } else {
        FC_SLANT_ROMAN as i32
    };

    let width = FC_WIDTH_NORMAL as f64;

    // Compute pixel size exactly matching FreeType's logic
    let nominal_pt = header.nominal_point_size as i64;
    let y_res = if header.vertical_resolution != 0 {
        header.vertical_resolution as i64
    } else {
        72
    };
    let size = nominal_pt << 6;
    let mut y_ppem = (size * y_res + 36) / 72;
    y_ppem = (y_ppem + 32) & !63;
    let max_y_ppem = (header.pixel_height as i64) << 6;
    if y_ppem > max_y_ppem {
        y_ppem = max_y_ppem;
    }
    let pixelsize = (y_ppem as f64) / 64.0;

    let family = if !header.face_name.is_empty() {
        header.face_name.to_string()
    } else {
        font_file
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string()
    };

    let decorative = contains_decorative(style);

    let mut pattern = FcPatternBuilder::new();

    let path_str = font_file.to_string_lossy().into_owned();
    if let Ok(cpath) = CString::new(path_str) {
        pattern.append_element(PatternElement::new(FC_FILE_OBJECT as i32, cpath.into()));
    }

    pattern.append_element(PatternElement::new(FC_INDEX_OBJECT as i32, (index as i32).into()));
    pattern.append_element(PatternElement::new(
        FC_FONTFORMAT_OBJECT as i32,
        CString::new("Windows FNT").unwrap().into(),
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

    // FreeType defaults to FC_SPACING_MONO when charmap selection fails for winfnt
    pattern.append_element(PatternElement::new(
        FC_SPACING_OBJECT as i32,
        (FC_SPACING_MONO as i32).into(),
    ));

    pattern.append_element(PatternElement::new(
        FC_FOUNDRY_OBJECT as i32,
        CString::new("unknown").unwrap().into(),
    ));

    pattern.append_element(PatternElement::new(
        FC_DECORATIVE_OBJECT as i32,
        decorative.into(),
    ));

    let charset = FcCharSetWrapper::new()?;
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

/// Builds Fontconfig patterns for all faces in a Windows FNT/FON font file.
pub fn build_patterns_for_winfnt(data: &[u8], font_file: &Path) -> Option<Vec<*mut FcPattern>> {
    let fnt_slices = extract_fnts(data);
    if fnt_slices.is_empty() {
        return None;
    }

    let mut patterns = Vec::new();
    for (index, slice) in fnt_slices.into_iter().enumerate() {
        if let Some(pattern) = build_pattern_for_face(slice, index, font_file) {
            patterns.push(pattern);
        }
    }

    if patterns.is_empty() {
        None
    } else {
        Some(patterns)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_winfnt() {
        assert!(!is_winfnt(b"OTTO"));
        assert!(!is_winfnt(b"STARTFONT 2.1"));
        assert!(!is_winfnt(b"\x01fcp"));
    }

    #[test]
    fn test_sanitize_ps_name() {
        assert_eq!(sanitize_ps_name("Fixedsys"), "Fixedsys");
        assert_eq!(sanitize_ps_name("MS Sans Serif"), "MS-Sans-Serif");
    }

    #[test]
    fn test_parse_synthetic_fnt() {
        let mut data = vec![0u8; 200];
        // version 0x300
        data[0] = 0x00;
        data[1] = 0x03;
        // file_size = 200
        data[2] = 200;
        // file_type = 0 (raster)
        data[66] = 0;
        // nominal_point_size = 10
        data[68] = 10;
        // vertical_resolution = 96
        data[70] = 96;
        // italic = 0
        data[80] = 0;
        // weight = 400
        data[83] = 0x90;
        data[84] = 0x01;
        // pixel_height = 13
        data[88] = 13;
        // face_name_offset = 150
        data[105] = 150;
        data[150..158].copy_from_slice(b"Courier\0");

        assert!(is_winfnt(&data));
        let header = WinFntHeader::parse(&data).expect("header parse failed");
        assert_eq!(header.face_name, "Courier");
        assert_eq!(header.nominal_point_size, 10);
        assert_eq!(header.pixel_height, 13);
        assert!(!header.italic);
    }
}
