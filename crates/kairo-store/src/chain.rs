//! Shared `supersedes`-chain head resolution for the per-record
//! materialized indexes.
//!
//! Branches, version tags, trust opinions, and capability grants
//! all share the same "pick the chain leaf, tiebreak on
//! `(created_at, statement_id)`" rule. Each per-index entry type
//! used to carry its own copy of the same algorithm; this module
//! collapses them to one.
//!
//! The algorithm:
//!   - Mark each entry as "superseded" if any sibling's
//!     `supersedes` field names it.
//!   - The leaves are entries that no sibling supersedes.
//!   - If exactly one leaf, that's the head.
//!   - If multiple leaves (a fork — same actor signed two
//!     statements both pointing at the same predecessor, or two
//!     genesis statements), tiebreak on `(created_at,
//!     statement_id)` descending. The greatest pair wins.
//!
//! Cross-actor `supersedes` resolution is deliberately out of
//! scope here: each per-record index keys entries by their
//! "subject" (actor, grantor, object), so a single
//! [`chain_head`] call only ever walks same-subject entries.
//! Cross-actor authority-aware walks live in `kairo-store`'s
//! `walk_authorized_*_chain` methods on top of this primitive
//! (using [`entry_greater_than`] as their per-pair tiebreak).
//!
//! Timestamp parsing is best-effort: a corrupted `created_at`
//! falls back to lexicographic `statement_id` compare so the
//! tiebreak still produces a deterministic answer. A subsequent
//! index read surfaces the corruption through the per-index
//! `into_*` decoders.

use std::collections::HashSet;

use kairo_core::Timestamp;

/// One entry in a `supersedes`-ordered chain.
///
/// Per-index entry types implement this in their own module and
/// pass their slice to [`chain_head`]. The trait carries only the
/// three fields chain resolution actually needs — the rest of the
/// entry's payload (decision text, branch name, scope, etc.) stays
/// in the per-index type.
pub(crate) trait ChainEntry {
    fn statement_id(&self) -> &str;
    fn supersedes(&self) -> Option<&str>;
    fn created_at(&self) -> &str;
}

/// Blanket impl so `&E: ChainEntry` whenever `E: ChainEntry`. The
/// cross-actor walkers in `lib.rs` iterate `&Vec<(&str, &Entry)>`
/// and therefore hand `&&Entry` to [`entry_greater_than`]; without
/// this impl, type inference would pick `E = &Entry` and fail on
/// the bound.
impl<E: ChainEntry + ?Sized> ChainEntry for &E {
    fn statement_id(&self) -> &str {
        (**self).statement_id()
    }

    fn supersedes(&self) -> Option<&str> {
        (**self).supersedes()
    }

    fn created_at(&self) -> &str {
        (**self).created_at()
    }
}

/// Pick the chain head from a set of entries. Returns `None` if
/// `entries` is empty. See the module doc for the full algorithm.
pub(crate) fn chain_head<E: ChainEntry>(entries: &[E]) -> Option<&E> {
    if entries.is_empty() {
        return None;
    }
    let superseded: HashSet<&str> = entries.iter().filter_map(|e| e.supersedes()).collect();
    let mut best: Option<&E> = None;
    for entry in entries {
        if superseded.contains(entry.statement_id()) {
            continue;
        }
        match best {
            None => best = Some(entry),
            Some(current) if entry_greater_than(entry, current) => best = Some(entry),
            _ => {}
        }
    }
    best
}

/// True iff `candidate` should beat `current` under the
/// `(created_at, statement_id)` descending tiebreak. Exposed
/// `pub(crate)` so the cross-actor authority walkers in `lib.rs`
/// can reuse the same per-pair primitive without rebuilding their
/// own chain folds.
///
/// The two arguments are independently generic so the cross-actor
/// walkers can pass mismatched reference depths (`&&Entry` from
/// the iterator vs `&Entry` from the `Option` destructure) without
/// extra reborrows at the call site. The blanket `ChainEntry for
/// &E` impl lets a `&Entry` participate as either argument.
pub(crate) fn entry_greater_than(candidate: &impl ChainEntry, current: &impl ChainEntry) -> bool {
    match (
        candidate.created_at().parse::<Timestamp>(),
        current.created_at().parse::<Timestamp>(),
    ) {
        (Ok(a), Ok(b)) => {
            if a > b {
                return true;
            }
            if a < b {
                return false;
            }
            candidate.statement_id() > current.statement_id()
        }
        // Corrupt timestamp on either side — fall back to
        // lexicographic statement-id compare so the tiebreak
        // remains deterministic. The corruption surfaces through
        // the per-index `into_*` decoders on the next read.
        _ => candidate.statement_id() > current.statement_id(),
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[derive(Debug, Clone)]
    struct Entry {
        statement_id: String,
        supersedes: Option<String>,
        created_at: String,
    }

    impl Entry {
        fn new(statement_id: &str, created_at: &str, supersedes: Option<&str>) -> Self {
            Self {
                statement_id: statement_id.to_owned(),
                supersedes: supersedes.map(str::to_owned),
                created_at: created_at.to_owned(),
            }
        }
    }

    impl ChainEntry for Entry {
        fn statement_id(&self) -> &str {
            &self.statement_id
        }

        fn supersedes(&self) -> Option<&str> {
            self.supersedes.as_deref()
        }

        fn created_at(&self) -> &str {
            &self.created_at
        }
    }

    fn ts(seconds: i64) -> String {
        Timestamp::from_seconds(seconds).to_string()
    }

    #[test]
    fn empty_returns_none() {
        let entries: Vec<Entry> = Vec::new();
        assert!(chain_head(&entries).is_none());
    }

    #[test]
    fn single_entry_is_the_head() {
        let entries = vec![Entry::new("a", &ts(100), None)];
        assert_eq!(
            chain_head(&entries).map(|e| e.statement_id.as_str()),
            Some("a")
        );
    }

    #[test]
    fn supersedes_chain_picks_the_leaf() {
        let entries = vec![
            Entry::new("a", &ts(100), None),
            Entry::new("b", &ts(100), Some("a")),
        ];
        assert_eq!(
            chain_head(&entries).map(|e| e.statement_id.as_str()),
            Some("b")
        );
    }

    #[test]
    fn fork_tiebreaks_on_later_timestamp() {
        let entries = vec![
            Entry::new("root", &ts(100), None),
            Entry::new("a", &ts(200), Some("root")),
            Entry::new("b", &ts(300), Some("root")),
        ];
        assert_eq!(
            chain_head(&entries).map(|e| e.statement_id.as_str()),
            Some("b")
        );
    }

    #[test]
    fn fork_with_equal_timestamps_tiebreaks_on_lex_statement_id() {
        let entries = vec![
            Entry::new("root", &ts(100), None),
            Entry::new("a", &ts(200), Some("root")),
            Entry::new("b", &ts(200), Some("root")),
        ];
        // Greatest (created_at, statement_id) wins; "b" > "a".
        assert_eq!(
            chain_head(&entries).map(|e| e.statement_id.as_str()),
            Some("b")
        );
    }

    #[test]
    fn corrupt_timestamp_falls_back_to_lex_compare() {
        let a = Entry::new("a", "not-an-rfc-3339", None);
        let b = Entry::new("b", "also-broken", None);
        // Both unparseable, both unsuperseded — lex pick.
        let entries = vec![a, b];
        assert_eq!(
            chain_head(&entries).map(|e| e.statement_id.as_str()),
            Some("b")
        );
    }

    #[test]
    fn supersedes_pointing_at_missing_predecessor_does_not_break_leaf() {
        // The successor names a `supersedes` id that isn't in the
        // entry slice (predecessor not yet imported, or signed by
        // a different actor and stored under a different key). The
        // successor is still a leaf — nothing in the slice
        // supersedes it — so it wins.
        let entries = vec![Entry::new(
            "successor",
            &ts(100),
            Some("missing-predecessor"),
        )];
        assert_eq!(
            chain_head(&entries).map(|e| e.statement_id.as_str()),
            Some("successor")
        );
    }

    #[test]
    fn entry_greater_than_matches_chain_head_tiebreak() {
        let a = Entry::new("a", &ts(100), None);
        let b = Entry::new("b", &ts(200), None);
        assert!(entry_greater_than(&b, &a));
        assert!(!entry_greater_than(&a, &b));
    }
}
