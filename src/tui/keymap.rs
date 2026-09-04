//! The keymap, in one place. SPEC §4.4.
//!
//! Written down once and read by the help screen, the config tab and the test
//! that checks every advertised key does something. v0.3.3 had it written
//! down twice — once in the help text, once in the handlers — which is how
//! the help screen came to teach `r  Refresh hosts` for a release after `r`
//! stopped refreshing anything.
//!
//! The descriptions are the ones the help screen prints, so a binding's
//! wording is part of the binding rather than a copy of it.

/// A binding, and what it does.
pub type Bind = (&'static str, &'static str);

/// SPEC §4.4. Six taught, down from ten.
///
/// F6, F7, F8 and F10 are still *bound* — they duplicate `Ctrl+A d`,
/// `Ctrl+A ←/→` and `Ctrl+P`, and unbinding a key somebody's fingers already
/// know is a regression with no upside. What v2 objects to is teaching ten
/// keys on a screen that should fit six, so they are bound and not listed.
/// Do not "fix" this by deleting the handlers.
pub const FUNCTION_KEYS: [Bind; 6] = [
    ("F1", "help"),
    ("F2", "monitor"),
    ("F3", "files"),
    ("F4", "forwards"),
    ("F5", "mini strip"),
    ("F9", "new session"),
];

/// Inside a session. Every one of these takes the prefix first.
pub const IN_SESSION: [Bind; 10] = [
    ("1-9", "jump to session N"),
    ("←/→", "previous / next"),
    ("Tab", "last session"),
    ("M", "terminal + monitor split"),
    ("m", "full-screen monitor"),
    ("f", "file browser"),
    ("p", "port forwards"),
    ("d", "detach, keep it running"),
    ("w", "close session"),
    ("t", "cycle theme"),
];

/// On the dashboard. Single keys, no prefix.
///
/// `q quit (sessions keep running)` states the persistence guarantee at the
/// point of anxiety, which is one of the three places SPEC §4.4 requires it.
pub const DASHBOARD: [Bind; 12] = [
    ("1-4", "switch tab"),
    ("j k ↑ ↓", "navigate list"),
    ("Tab", "next panel"),
    ("Enter", "connect / attach"),
    ("n", "new session here"),
    ("d", "detach session"),
    ("r", "reconnect / retry host"),
    ("m", "monitor selected host"),
    ("a", "add host"),
    ("e", "edit host"),
    ("/", "search hosts"),
    ("q", "quit (sessions keep running)"),
];

/// The widest key label in a column, which sets that column's key width.
pub fn key_width(binds: &[Bind]) -> usize {
    binds
        .iter()
        .map(|(k, _)| crate::design::width_of(k))
        .max()
        .unwrap_or(0)
}

/// The widest description, which is what SPEC §3.5's column budget has to
/// clear. `quit (sessions keep running)` is the one that sets it, and the
/// handoff's own `1.25fr 1fr` split leaves 24 cells for a 28-cell string.
///
/// Used by the budget assertion rather than by the renderer: the column fills
/// whatever it is given, and this is how we know what it has to be given.
#[cfg(test)]
pub fn desc_width(binds: &[Bind]) -> usize {
    binds
        .iter()
        .map(|(_, d)| crate::design::width_of(d))
        .max()
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC §3.5, and the reason the budget is written down.
    ///
    /// The handoff splits the help screen `1.25fr 1fr`, which leaves 24 cells
    /// for a description column whose longest entry is 28. That is defect 4
    /// of the v0.3.3 list — help wrapping mid-phrase — rebuilt at a different
    /// width, and it is why this asserts the content against the budget
    /// rather than trusting the ratio.
    #[test]
    fn the_column_budget_clears_the_longest_binding() {
        // Two labelled columns inside a 2fr panel at 150 columns: the panel
        // is 96 wide, 92 inside its border, 45 per column after the gap.
        const COLUMN: usize = 45;
        for binds in [&IN_SESSION[..], &DASHBOARD[..]] {
            let needed = key_width(binds) + 2 + desc_width(binds);
            assert!(
                needed <= COLUMN,
                "the widest binding needs {needed} cells in a {COLUMN}-cell column"
            );
        }
    }

    /// Six function keys, not ten. The four that went were duplicates of
    /// prefix bindings that already existed.
    #[test]
    fn the_function_row_is_six_keys() {
        assert_eq!(FUNCTION_KEYS.len(), 6);
        for (key, _) in FUNCTION_KEYS {
            assert!(key.starts_with('F'), "{key} is not a function key");
        }
    }

    /// SPEC §4.3. `r` spent a release refusing to act because v0.3.3
    /// advertised it as probe and performed a reload. It has one meaning now,
    /// and the screen that teaches the bindings has to be the first place
    /// that is true.
    #[test]
    fn r_is_advertised_as_one_thing() {
        let r = DASHBOARD
            .iter()
            .find(|(k, _)| *k == "r")
            .expect("r is bound");
        assert_eq!(r.1, "reconnect / retry host");
        for (_, desc) in DASHBOARD {
            assert!(
                !desc.contains("probe"),
                "the dashboard still advertises a probe key: {desc}"
            );
        }
    }

    /// The persistence guarantee, at the point of anxiety.
    #[test]
    fn quit_says_what_happens_to_the_sessions() {
        let q = DASHBOARD
            .iter()
            .find(|(k, _)| *k == "q")
            .expect("q is bound");
        assert!(q.1.contains("keep running"), "{}", q.1);
    }

    /// No key means two things in one place. `d` detaches on the dashboard
    /// and detaches in a session; `m` monitors in both. That is the same verb
    /// twice, which is fine. Two different verbs on one key is not.
    #[test]
    fn no_key_is_bound_twice_in_one_context() {
        for binds in [&IN_SESSION[..], &DASHBOARD[..], &FUNCTION_KEYS[..]] {
            let mut seen: Vec<&str> = Vec::new();
            for (key, _) in binds {
                assert!(!seen.contains(key), "`{key}` is bound twice in one column");
                seen.push(key);
            }
        }
    }
}
