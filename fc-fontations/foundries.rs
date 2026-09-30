/*
 * fontconfig/fc-fontations/foundries.rs
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

use read_fonts::TableProvider;
use skrifa::{
    string::{LocalizedStrings, StringId},
    FontRef, MetadataProvider,
};
use std::ffi::{CStr, CString};

/// Foundries recognised by a substring of a font's notice or manufacturer string.
///
/// Mirrors `FcNoticeFoundries` in `fcfoundry.h`.
pub const NOTICE_FOUNDRIES: [(&str, &str); 18] = [
    ("Adobe", "adobe"),
    ("Bigelow", "b&h"),
    ("Bitstream", "bitstream"),
    ("Gnat", "culmus"),
    ("Iorsh", "culmus"),
    ("HanYang System", "hanyang"),
    ("Font21", "hwan"),
    ("IBM", "ibm"),
    ("International Typeface Corporation", "itc"),
    ("Linotype", "linotype"),
    ("LINOTYPE-HELL", "linotype"),
    ("Microsoft", "microsoft"),
    ("Monotype", "monotype"),
    ("Omega", "omega"),
    ("Tiro Typeworks", "tiro"),
    ("URW", "urw"),
    ("XFree86", "xfree86"),
    ("Xorg", "xorg"),
];

/// Finds a recognised foundry by substring match in a font's notice or manufacturer string.
///
/// Mirrors `FcNoticeFoundry` in `src/fcfreetype.c`.
pub fn notice_to_foundry(notice: &str) -> Option<&'static str> {
    NOTICE_FOUNDRIES
        .iter()
        .find(|(pattern, _)| notice.contains(pattern))
        .map(|(_, foundry)| *foundry)
}

fn map_foundry_from_name_entry(localized_strings: &mut LocalizedStrings) -> Option<CString> {
    localized_strings.into_iter().find_map(|foundry_name| {
        notice_to_foundry(foundry_name.to_string().as_str())
            .map(|foundry| CString::new(foundry).unwrap())
    })
}

pub fn make_foundry(font: &FontRef) -> Option<CString> {
    if let Ok(os2) = font.os2() {
        let vend_bytes = os2.ach_vend_id().to_be_bytes();
        let foundry = if vend_bytes.contains(&0) {
            CStr::from_bytes_until_nul(&vend_bytes)
                .ok()
                .map(|cstr| cstr.to_owned())
        } else {
            CString::new(vend_bytes).ok()
        };

        if let Some(foundry) = foundry {
            if !foundry.is_empty() {
                return Some(foundry);
            }
        }
    }

    map_foundry_from_name_entry(&mut font.localized_strings(StringId::TRADEMARK)).or_else(|| {
        map_foundry_from_name_entry(&mut font.localized_strings(StringId::MANUFACTURER))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_notice_to_foundry() {
        assert_eq!(
            notice_to_foundry("Adobe Systems Incorporated"),
            Some("adobe")
        );
        assert_eq!(notice_to_foundry("Bigelow & Holmes"), Some("b&h"));
        assert_eq!(notice_to_foundry("Bitstream Inc."), Some("bitstream"));
        assert_eq!(notice_to_foundry("Copyright (c) URW++ Design"), Some("urw"));
        assert_eq!(
            notice_to_foundry("Microsoft Corporation"),
            Some("microsoft")
        );
        assert_eq!(notice_to_foundry("Unknown Foundry"), None);
    }
}
