//! Language of the texts hearth shows: German when Windows shows German,
//! English otherwise. `HEARTH_LANG` (`de` or `en`) overrides the
//! detection.
//!
//! Both texts stay side by side where they are used, without a key table:
//! [`tr`] picks one of two fixed texts, [`tr!`](crate::tr) formats one of
//! two texts with the same arguments. Texts for the web chat do not go
//! through here; the web app translates on its own.
//!
//! Tests default to German, so their expectations do not depend on the
//! Windows of whoever runs them: `cfg(test)` for this crate, the feature
//! `test-german` for the tests of crates that depend on it.

use std::sync::OnceLock;

/// A language of the user interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    /// German.
    De,
    /// English.
    En,
}

impl Lang {
    /// `de` for German, `en` for English.
    pub fn pick<T>(self, de: T, en: T) -> T {
        match self {
            Lang::De => de,
            Lang::En => en,
        }
    }
}

/// The language of this process, detected on first use.
pub fn current() -> Lang {
    static CURRENT: OnceLock<Lang> = OnceLock::new();
    *CURRENT.get_or_init(|| select(std::env::var("HEARTH_LANG").ok().as_deref(), default_lang))
}

/// Whether texts are German.
pub fn german() -> bool {
    current() == Lang::De
}

/// `de` or `en`, whichever the user reads. For text with arguments, see
/// [`tr!`](crate::tr).
pub fn tr<T>(de: T, en: T) -> T {
    current().pick(de, en)
}

/// The language `HEARTH_LANG` names (`de`, `en`, also `de-AT` and the
/// like, in any case), else `fallback()`.
fn select(value: Option<&str>, fallback: fn() -> Lang) -> Lang {
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
        "de" => Lang::De,
        "en" => Lang::En,
        _ => fallback(),
    }
}

/// German in tests, so that their expected texts hold on any Windows;
/// otherwise the language of the system.
fn default_lang() -> Lang {
    if cfg!(any(test, feature = "test-german")) {
        Lang::De
    } else {
        system()
    }
}

/// German when the Windows UI language is German (primary language
/// 0x07), English for every other language.
#[cfg(windows)]
fn system() -> Lang {
    // SAFETY: no arguments; reads the user's settings.
    let id = unsafe { windows_sys::Win32::Globalization::GetUserDefaultUILanguage() };
    if id & 0x3ff == 0x07 {
        Lang::De
    } else {
        Lang::En
    }
}

/// German when the locale variables name German, as POSIX orders them.
#[cfg(not(windows))]
fn system() -> Lang {
    let locale = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .find_map(|name| std::env::var(name).ok().filter(|v| !v.is_empty()));
    if locale.is_some_and(|v| v.starts_with("de")) {
        Lang::De
    } else {
        Lang::En
    }
}

/// Formats German or English text, whichever the user reads, like
/// `format!`: `tr!("Projekt {name} angelegt", "Project {name} created")`.
/// Both texts must use all arguments. For fixed text, [`lang::tr`] avoids
/// the allocation.
///
/// `tr!(in lang; "…", "…", …)` formats in the given [`Lang`] instead,
/// for text whose reader is not the user at this terminal (the web chat).
///
/// [`lang::tr`]: crate::lang::tr
/// [`Lang`]: crate::lang::Lang
#[macro_export]
macro_rules! tr {
    (in $lang:expr; $de:literal, $en:literal $(, $($args:tt)+)?) => {
        match $lang {
            $crate::lang::Lang::De => ::std::format!($de $(, $($args)+)?),
            $crate::lang::Lang::En => ::std::format!($en $(, $($args)+)?),
        }
    };
    ($de:literal, $en:literal $(,)?) => {
        if $crate::lang::german() {
            ::std::format!($de)
        } else {
            ::std::format!($en)
        }
    };
    ($de:literal, $en:literal, $($args:tt)+) => {
        if $crate::lang::german() {
            ::std::format!($de, $($args)+)
        } else {
            ::std::format!($en, $($args)+)
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hearth_lang_overrides_windows() {
        let german = || Lang::De;
        let english = || Lang::En;
        assert_eq!(select(Some("en"), german), Lang::En);
        assert_eq!(select(Some("EN"), german), Lang::En);
        assert_eq!(select(Some("en-US"), german), Lang::En);
        assert_eq!(select(Some("de"), english), Lang::De);
        assert_eq!(select(Some(" de_AT "), english), Lang::De);
    }

    #[test]
    fn without_a_known_language_windows_decides() {
        assert_eq!(select(None, || Lang::En), Lang::En);
        assert_eq!(select(None, || Lang::De), Lang::De);
        assert_eq!(select(Some(""), || Lang::En), Lang::En);
        assert_eq!(select(Some("fr"), || Lang::En), Lang::En);
        assert_eq!(select(Some("fr"), || Lang::De), Lang::De);
    }

    #[test]
    fn tests_read_german_unless_told_otherwise() {
        assert_eq!(default_lang(), Lang::De);
        let expected = select(std::env::var("HEARTH_LANG").ok().as_deref(), || Lang::De);
        assert_eq!(current(), expected);
    }

    #[test]
    fn both_texts_take_the_same_arguments() {
        let name = "demo";
        let lang = current();
        assert_eq!(
            crate::tr!("Projekt {name} angelegt", "Project {name} created"),
            lang.pick("Projekt demo angelegt", "Project demo created")
        );
        assert_eq!(
            crate::tr!("{} von {}", "{} of {}", 1, 2),
            lang.pick("1 von 2", "1 of 2")
        );
        assert_eq!(
            crate::tr!("Fertig.", "Done."),
            lang.pick("Fertig.", "Done.")
        );
        assert_eq!(tr("ja", "yes"), lang.pick("ja", "yes"));
        assert_eq!(
            crate::tr!(in Lang::En; "Projekt {name}", "Project {name}"),
            "Project demo"
        );
        assert_eq!(crate::tr!(in Lang::De; "fertig", "done"), "fertig");
    }
}
