/*
 * fontconfig/fc-fontations/style_consts.rs
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

//! Style mapping constants for weight, width, slant, and decorative traits.
//!
//! Mirrors Fontconfig's `weightConsts`, `widthConsts`, `slantConsts`, and
//! `decorativeConsts` from `src/fcfreetype.c`.

use fontconfig_bindings::{
    FC_SLANT_ITALIC, FC_SLANT_OBLIQUE, FC_WEIGHT_BLACK, FC_WEIGHT_BOLD, FC_WEIGHT_BOOK,
    FC_WEIGHT_DEMIBOLD, FC_WEIGHT_DEMILIGHT, FC_WEIGHT_EXTRABLACK, FC_WEIGHT_EXTRABOLD,
    FC_WEIGHT_EXTRALIGHT, FC_WEIGHT_HEAVY, FC_WEIGHT_LIGHT, FC_WEIGHT_MEDIUM, FC_WEIGHT_NORMAL,
    FC_WEIGHT_REGULAR, FC_WEIGHT_SEMIBOLD, FC_WEIGHT_SEMILIGHT, FC_WEIGHT_THIN,
    FC_WEIGHT_ULTRABLACK, FC_WEIGHT_ULTRABOLD, FC_WEIGHT_ULTRALIGHT, FC_WIDTH_CONDENSED,
    FC_WIDTH_EXPANDED, FC_WIDTH_EXTRACONDENSED, FC_WIDTH_EXTRAEXPANDED, FC_WIDTH_NORMAL,
    FC_WIDTH_SEMICONDENSED, FC_WIDTH_SEMIEXPANDED, FC_WIDTH_ULTRACONDENSED, FC_WIDTH_ULTRAEXPANDED,
};

/// Weight constants mapping lowercase names to Fontconfig weight values.
///
/// Mirrors `weightConsts` in `src/fcfreetype.c`. Entries prefixed with `'<'`
/// indicate whole-word matching in FreeType.
pub static WEIGHT_CONSTS: &[(&str, f64)] = &[
    ("thin", FC_WEIGHT_THIN as f64),
    ("extralight", FC_WEIGHT_EXTRALIGHT as f64),
    ("ultralight", FC_WEIGHT_ULTRALIGHT as f64),
    ("demilight", FC_WEIGHT_DEMILIGHT as f64),
    ("semilight", FC_WEIGHT_SEMILIGHT as f64),
    ("light", FC_WEIGHT_LIGHT as f64),
    ("book", FC_WEIGHT_BOOK as f64),
    ("regular", FC_WEIGHT_REGULAR as f64),
    ("normal", FC_WEIGHT_NORMAL as f64),
    ("medium", FC_WEIGHT_MEDIUM as f64),
    ("demibold", FC_WEIGHT_DEMIBOLD as f64),
    ("demi", FC_WEIGHT_DEMIBOLD as f64),
    ("semibold", FC_WEIGHT_SEMIBOLD as f64),
    ("extrabold", FC_WEIGHT_EXTRABOLD as f64),
    ("superbold", FC_WEIGHT_EXTRABOLD as f64),
    ("ultrabold", FC_WEIGHT_ULTRABOLD as f64),
    ("bold", FC_WEIGHT_BOLD as f64),
    ("ultrablack", FC_WEIGHT_ULTRABLACK as f64),
    ("superblack", FC_WEIGHT_EXTRABLACK as f64),
    ("extrablack", FC_WEIGHT_EXTRABLACK as f64),
    ("<ultra", FC_WEIGHT_ULTRABOLD as f64),
    ("black", FC_WEIGHT_BLACK as f64),
    ("heavy", FC_WEIGHT_HEAVY as f64),
];

/// Width constants mapping lowercase names to Fontconfig width values.
///
/// Mirrors `widthConsts` in `src/fcfreetype.c`.
pub static WIDTH_CONSTS: &[(&str, f64)] = &[
    ("ultracondensed", FC_WIDTH_ULTRACONDENSED as f64),
    ("extracondensed", FC_WIDTH_EXTRACONDENSED as f64),
    ("semicondensed", FC_WIDTH_SEMICONDENSED as f64),
    ("condensed", FC_WIDTH_CONDENSED as f64),
    ("normal", FC_WIDTH_NORMAL as f64),
    ("semiexpanded", FC_WIDTH_SEMIEXPANDED as f64),
    ("extraexpanded", FC_WIDTH_EXTRAEXPANDED as f64),
    ("ultraexpanded", FC_WIDTH_ULTRAEXPANDED as f64),
    ("expanded", FC_WIDTH_EXPANDED as f64),
    ("extended", FC_WIDTH_EXPANDED as f64),
];

/// Slant constants mapping lowercase names to Fontconfig slant values.
///
/// Mirrors `slantConsts` in `src/fcfreetype.c`.
pub static SLANT_CONSTS: &[(&str, i32)] = &[
    ("italic", FC_SLANT_ITALIC as i32),
    ("kursiv", FC_SLANT_ITALIC as i32),
    ("oblique", FC_SLANT_OBLIQUE as i32),
];

/// Decorative traits recognized from style names.
///
/// Mirrors `decorativeConsts` in `src/fcfreetype.c`.
pub static DECORATIVE_CONSTS: &[&str] =
    &["shadow", "caps", "antiqua", "romansc", "embosed", "dunhill"];

fn blanks_removed(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect()
}

fn contains_word(text: &str, search_word: &str) -> bool {
    let words = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty());

    for word in words {
        if word.eq_ignore_ascii_case(search_word) {
            return true;
        }
    }
    false
}

/// Checks whether `name` exactly matches a known weight constant (ignoring blanks and case).
///
/// Mirrors `FcIsWeight` in `src/fcfreetype.c`.
#[allow(dead_code)]
pub fn is_weight(name: &str) -> Option<f64> {
    let name_folded = blanks_removed(name);
    WEIGHT_CONSTS
        .iter()
        .find(|(known_weight, _)| !known_weight.starts_with('<') && name_folded == *known_weight)
        .map(|(_, weight)| *weight)
}

/// Searches `style` for a known weight constant (ignoring blanks and case, respecting word boundaries for `<` entries).
///
/// Mirrors `FcContainsWeight` in `src/fcfreetype.c`.
pub fn contains_weight(style: &str) -> Option<f64> {
    let folded = blanks_removed(style);
    WEIGHT_CONSTS
        .iter()
        .find(|(known_weight, _)| match known_weight.strip_prefix('<') {
            Some(search_word) => contains_word(style, search_word),
            None => folded.contains(known_weight),
        })
        .map(|(_, weight)| *weight)
}

/// Searches `style` for a known slant constant.
///
/// Mirrors `FcContainsSlant` in `src/fcfreetype.c`.
pub fn contains_slant(style: &str) -> Option<i32> {
    let folded = blanks_removed(style);
    SLANT_CONSTS
        .iter()
        .find(|(known_slant, _)| folded.contains(known_slant))
        .map(|(_, slant)| *slant)
}

/// Searches `style` for a known width constant.
///
/// Mirrors `FcContainsWidth` in `src/fcfreetype.c`.
pub fn contains_width(style: &str) -> Option<f64> {
    let folded = blanks_removed(style);
    WIDTH_CONSTS
        .iter()
        .find(|(known_width, _)| folded.contains(known_width))
        .map(|(_, width)| *width)
}

/// Checks whether `style` contains a recognized decorative trait.
///
/// Mirrors `FcContainsDecorative` in `src/fcfreetype.c`.
pub fn contains_decorative(style: &str) -> bool {
    let folded = blanks_removed(style);
    DECORATIVE_CONSTS
        .iter()
        .any(|&decorative| folded.contains(decorative))
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(contains_width("Condensed"), Some(75.0));
        assert_eq!(contains_width("Bold Condensed"), Some(75.0));
        assert_eq!(contains_width("UltraExpanded"), Some(200.0));
        assert_eq!(contains_width("Regular"), None);
    }

    #[test]
    fn test_decorative() {
        assert!(contains_decorative("Shadow"));
        assert!(contains_decorative("Caps Roman"));
        assert!(contains_decorative("Antiqua"));
        assert!(!contains_decorative("Regular"));
    }

    #[test]
    fn test_contains_word() {
        assert!(contains_word("Ultra Bold", "ultra"));
        assert!(contains_word("Ultra-Bold", "ultra"));
        assert!(contains_word("Bold Ultra", "ultra"));
        assert!(contains_word("Ultra", "ultra"));
        assert!(!contains_word("UltraBold", "ultra"));
        assert!(!contains_word("SuperUltra", "ultra"));
        assert!(!contains_word("Ultracool", "ultra"));
        assert!(!contains_word("", "ultra"));
    }
}
