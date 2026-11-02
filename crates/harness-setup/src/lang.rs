//! German or English texts, as in `harness_core::lang`: German when the
//! Windows UI language is German, English otherwise; `HARNESS_LANG` (`de`
//! or `en`) overrides it, and tests default to German. A copy, because the
//! installer carries harness instead of linking it.

use std::sync::OnceLock;

/// Whether texts are German; detected once.
pub fn german() -> bool {
    static GERMAN: OnceLock<bool> = OnceLock::new();
    *GERMAN.get_or_init(|| {
        select(
            std::env::var("HARNESS_LANG").ok().as_deref(),
            default_german,
        )
    })
}

/// `de` or `en`, whichever the user reads.
pub fn tr<T>(de: T, en: T) -> T {
    if german() { de } else { en }
}

/// Whether `HARNESS_LANG` names German (`de`, `de-AT`, …) or English, else
/// `fallback()`.
fn select(value: Option<&str>, fallback: fn() -> bool) -> bool {
    let primary = value
        .map(|v| {
            v.trim()
                .split(['-', '_'])
                .next()
                .unwrap_or("")
                .to_ascii_lowercase()
        })
        .unwrap_or_default();
    match primary.as_str() {
        "de" => true,
        "en" => false,
        _ => fallback(),
    }
}

/// German in tests, otherwise the language of Windows.
fn default_german() -> bool {
    cfg!(test) || system_german()
}

/// Whether the Windows UI language is German (primary language 0x07).
#[cfg(windows)]
fn system_german() -> bool {
    // SAFETY: no arguments; reads the user's settings.
    let id = unsafe { windows_sys::Win32::Globalization::GetUserDefaultUILanguage() };
    id & 0x3ff == 0x07
}

#[cfg(not(windows))]
fn system_german() -> bool {
    false
}

/// Formats German or English text like `format!`; both texts must use all
/// arguments.
macro_rules! tr {
    ($de:literal, $en:literal $(, $($args:tt)+)?) => {
        if $crate::lang::german() {
            ::std::format!($de $(, $($args)+)?)
        } else {
            ::std::format!($en $(, $($args)+)?)
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn harness_lang_overrides_windows() {
        assert!(!select(Some("en"), || true));
        assert!(!select(Some("EN-us"), || true));
        assert!(select(Some("de"), || false));
        assert!(select(Some(" de_AT "), || false));
        assert!(select(None, || true));
        assert!(!select(Some("fr"), || false));
        assert!(select(Some(""), || true));
    }

    #[test]
    fn tests_read_german_unless_told_otherwise() {
        assert!(default_german());
        let expected = select(std::env::var("HARNESS_LANG").ok().as_deref(), || true);
        assert_eq!(german(), expected);
        let version = "1.0";
        assert_eq!(
            tr!("Version {version} fertig", "Version {version} done"),
            tr("Version 1.0 fertig", "Version 1.0 done")
        );
    }
}
