/*
 * fontconfig/fc-fontations/postscript.rs
 *
 * Copyright 2025-2026 Google LLC.
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

//! PostScript header parsing helpers for Type 1 fonts.

/// Finds the first occurrence of `key` in `data` that is not preceded by a slash,
/// returning the slice immediately following the key.
///
/// In PostScript syntax, a single slash (e.g. `/isFixedPitch`) introduces a literal
/// name, while a double slash (e.g. `//isFixedPitch`) introduces an immediately evaluated
/// name. This helper scans for `key` and skips matches preceded by `/`.
fn find_key<'a>(data: &'a [u8], key: &[u8]) -> Option<&'a [u8]> {
    data.windows(key.len())
        .enumerate()
        .find(|&(pos, window)| window == key && (pos == 0 || data[pos - 1] != b'/'))
        .map(|(pos, _)| &data[pos + key.len()..])
}

/// Extracts a balanced PostScript literal string value associated with `key`.
///
/// PostScript string literals are enclosed in matching parentheses `(` and `)`
/// and may contain nested balanced parentheses or escaped characters.
///
/// ### Example:
/// Given PostScript data such as:
/// ```text
/// /Notice (Copyright 1990 Bitstream Inc.) def
/// /Notice ((URW)++, Copyright 2013 by (URW)++ Design & Development) readonly def
/// /Notice (Copyright \(c\) 1989 Adobe Systems Incorporated.) def
/// ```
/// Looking up `key = b"/Notice"` locates `/Notice`, finds the opening `(`,
/// and extracts the inner string slice up to the matching closing `)`.
pub fn postscript_string<'a>(data: &'a [u8], key: &[u8]) -> Option<&'a str> {
    let after_key = find_key(data, key)?;
    let open_pos = after_key.iter().position(|&b| b == b'(')?;
    let inner = &after_key[open_pos + 1..];

    let mut depth = 1usize;
    let mut escaped = false;
    for (idx, &byte) in inner.iter().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }
        match byte {
            b'\\' => escaped = true,
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return std::str::from_utf8(&inner[..idx]).ok();
                }
            }
            _ => {}
        }
    }
    None
}

/// Checks whether a boolean flag `key` is set to `true` in a PostScript header.
///
/// Looks for `key` in the first 64 KB of `data` (skipping double-slash tokens like `//key`),
/// strips following whitespace, and checks if the value begins with `true` on a token boundary.
///
/// ### Example:
/// In a Type 1 font dictionary:
/// ```text
/// /isFixedPitch true def
/// /isFixedPitch false def
/// ```
/// Calling `postscript_flag(data, b"/isFixedPitch")` returns `true` for the first
/// snippet, and `false` for the second (or when the key is absent or preceded by double slashes).
pub fn postscript_flag(data: &[u8], key: &[u8]) -> bool {
    let header = &data[..data.len().min(64 * 1024)];
    let Some(rest) = find_key(header, key) else {
        return false;
    };
    let trimmed = rest
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .map(|start| &rest[start..])
        .unwrap_or(b"");
    trimmed.starts_with(b"true") && trimmed.get(4).is_none_or(|b| !b.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_postscript_string() {
        let data = b"/Notice (Copyright 2026) def /Other (test)";
        assert_eq!(postscript_string(data, b"/Notice"), Some("Copyright 2026"));
        assert_eq!(postscript_string(data, b"/Other"), Some("test"));
        assert_eq!(postscript_string(data, b"/NonExistent"), None);

        // Nested parentheses
        let nested = b"/Notice ((URW)++, Copyright 2013) readonly def";
        assert_eq!(
            postscript_string(nested, b"/Notice"),
            Some("(URW)++, Copyright 2013")
        );

        // Escaped parentheses
        let escaped = b"/Notice (Copyright \\(c\\) 1999 Adobe Systems) def";
        assert_eq!(
            postscript_string(escaped, b"/Notice"),
            Some("Copyright \\(c\\) 1999 Adobe Systems")
        );

        // Empty string
        assert_eq!(postscript_string(b"/Notice () def", b"/Notice"), Some(""));

        // Unterminated string
        assert_eq!(postscript_string(b"/Notice (Unclosed", b"/Notice"), None);

        // Double slash: should skip //Notice and match /Notice if present, or return None
        assert_eq!(
            postscript_string(b"//Notice (double slash)", b"/Notice"),
            None
        );
        let double_then_single = b"//Notice (ignored) def /Notice (valid) def";
        assert_eq!(
            postscript_string(double_then_single, b"/Notice"),
            Some("valid")
        );
    }

    #[test]
    fn test_postscript_flag() {
        let data = b"/isFixedPitch true def /Other false def";
        assert!(postscript_flag(data, b"/isFixedPitch"));
        assert!(!postscript_flag(data, b"/Other"));
        assert!(!postscript_flag(data, b"/NonExistent"));

        // Flag followed by non-alphanumeric token boundary
        assert!(postscript_flag(
            b"/isFixedPitch true\ndef",
            b"/isFixedPitch"
        ));
        assert!(postscript_flag(
            b"/isFixedPitch true\r\ndef",
            b"/isFixedPitch"
        ));
        assert!(postscript_flag(b"/isFixedPitch true", b"/isFixedPitch"));

        // Not a true boolean: e.g. "truetype"
        assert!(!postscript_flag(
            b"/isFixedPitch truetype def",
            b"/isFixedPitch"
        ));

        // Double slashes: //isFixedPitch should not match
        assert!(!postscript_flag(
            b"//isFixedPitch true def",
            b"/isFixedPitch"
        ));

        // Double slash followed by actual single slash
        let double_and_single = b"//isFixedPitch false def /isFixedPitch true def";
        assert!(postscript_flag(double_and_single, b"/isFixedPitch"));

        let double_and_single_false = b"//isFixedPitch true def /isFixedPitch false def";
        assert!(!postscript_flag(double_and_single_false, b"/isFixedPitch"));

        // Non-existence
        assert!(!postscript_flag(b"", b"/isFixedPitch"));
        assert!(!postscript_flag(
            b"/SomeOtherKey true def",
            b"/isFixedPitch"
        ));
    }
}
