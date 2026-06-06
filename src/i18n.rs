//! Trivial string-table i18n.
//!
//! For v0.1: a tiny lookup table (zh <-> en).  The Slint side also carries
//! @tr(...) markers which are resolved at compile-time from .po files.
//! This module only handles Rust-side dynamic strings.

use std::cell::Cell;

thread_local! {
    static LANG: Cell<&'static str> = const { Cell::new("zh") };
}

pub fn set_language(code: &str) {
    LANG.set(if code == "en" { "en" } else { "zh" });
}

pub fn is_en() -> bool {
    LANG.get() == "en"
}

pub fn current_code() -> &'static str {
    LANG.get()
}

/// Simple lookup: pass zh, en and get the active language's string.
pub fn t(zh: &'static str, en: &'static str) -> &'static str {
    if is_en() { en } else { zh }
}

/// Apply the current Rust-side language to Slint's bundled-translation system.
#[allow(unexpected_cfgs)]
pub fn apply_to_slint() {
    // Only available when translations are compiled into Slint.
    #[cfg(feature = "slint/unstable-translations")]
    let _ = slint::select_bundled_translation(current_code());
    #[cfg(not(feature = "slint/unstable-translations"))]
    let _ = ();
}