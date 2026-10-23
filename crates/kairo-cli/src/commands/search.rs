//! `kairo search` (`specs/DECISIONS.md` §44 sub-decision 3): native
//! objects at their current snapshots and their children, by name,
//! summary, tags, and provides labels. No index: each query reads what
//! the resolver's index reads. Results are candidates, never claims of
//! validity (`specs/SEARCH.md` §2).

use std::fmt::Write as _;

use kairo_core::ObjectId;
use kairo_resolver::Index;

use crate::commands::build::CacheManifests;
use crate::error::CliError;
use crate::store_paths::{open_store, StorePaths};

/// A parsed query: free words, and the filters `SEARCH.md` §5 names.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Query {
    words: Vec<String>,
    tags: Vec<String>,
    provides: Vec<Provides>,
    /// Filters this node doesn't know, ignored and reported.
    ignored: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
enum Provides {
    Label(String),
    /// `provides:<kind>:*`: every label of that kind.
    Kind(String),
}

impl Query {
    pub(crate) fn parse(terms: &[String]) -> Self {
        let mut query = Self::default();
        for term in terms.iter().flat_map(|term| term.split_whitespace()) {
            match term.split_once(':') {
                Some(("tag", tag)) if !tag.is_empty() => query.tags.push(tag.to_lowercase()),
                Some(("provides", label)) if !label.is_empty() => {
                    query.provides.push(match label.strip_suffix(":*") {
                        Some(kind) => Provides::Kind(kind.to_owned()),
                        None => Provides::Label(label.to_owned()),
                    });
                }
                Some((key, _)) if !key.is_empty() => query.ignored.push(term.to_owned()),
                _ => query.words.push(term.to_lowercase()),
            }
        }
        query
    }
}

/// One searchable thing: a native object or a child.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Entry {
    pub(crate) object: ObjectId,
    pub(crate) snapshot: String,
    pub(crate) name: String,
    pub(crate) summary: Option<String>,
    pub(crate) tags: Vec<String>,
    /// Each label with its version.
    pub(crate) provides: Vec<(String, Option<String>)>,
    /// A child's own version.
    pub(crate) version: Option<String>,
    /// A child's parent.
    pub(crate) parent: Option<ObjectId>,
}

/// Every object the index holds, and each of their children.
pub(crate) fn entries(index: &Index) -> Vec<Entry> {
    let mut entries = Vec::new();
    for candidate in &index.candidates {
        let manifest = &candidate.manifest;
        entries.push(Entry {
            object: candidate.object.clone(),
            snapshot: candidate.snapshot.to_string(),
            name: manifest.kairo().name().to_owned(),
            summary: manifest.kairo().summary().map(str::to_owned),
            tags: manifest.kairo().tags().to_vec(),
            provides: manifest
                .provides()
                .iter()
                .map(|p| (p.provides().to_owned(), p.version().map(str::to_owned)))
                .collect(),
            version: None,
            parent: None,
        });
        for target in manifest.build_targets() {
            for child in target.children() {
                entries.push(Entry {
                    object: kairo_object::ChildGenesis::new(
                        candidate.object.clone(),
                        target.name(),
                        child.name(),
                    )
                    .object_id(),
                    snapshot: candidate.snapshot.to_string(),
                    name: child.name().to_owned(),
                    summary: child.summary().map(str::to_owned),
                    tags: child.tags().to_vec(),
                    provides: child
                        .provides()
                        .iter()
                        .map(|p| (p.provides().to_owned(), p.version().map(str::to_owned)))
                        .collect(),
                    version: child.version().map(str::to_owned),
                    parent: Some(candidate.object.clone()),
                });
            }
        }
    }
    entries
}

/// The entries `query` matches, those whose name or a tag a word names
/// exactly first, then by name and id.
pub(crate) fn search(entries: Vec<Entry>, query: &Query) -> Vec<Entry> {
    let mut found: Vec<(usize, Entry)> = entries
        .into_iter()
        .filter(|entry| matches(entry, query))
        .map(|entry| {
            let exact = query
                .words
                .iter()
                .filter(|word| {
                    entry.name.to_lowercase() == **word || entry.tags.iter().any(|tag| tag == *word)
                })
                .count();
            (exact, entry)
        })
        .collect();
    found.sort_by(|(a_exact, a), (b_exact, b)| {
        b_exact
            .cmp(a_exact)
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.object.cmp(&b.object))
    });
    found.into_iter().map(|(_, entry)| entry).collect()
}

fn matches(entry: &Entry, query: &Query) -> bool {
    let text = [
        entry.name.to_lowercase(),
        entry.summary.as_deref().unwrap_or_default().to_lowercase(),
        entry.tags.join(" "),
    ]
    .join(" ");
    query.words.iter().all(|word| text.contains(word.as_str()))
        && query.tags.iter().all(|tag| entry.tags.contains(tag))
        && query.provides.iter().all(|wanted| {
            entry.provides.iter().any(|(label, _)| match wanted {
                Provides::Label(wanted) => label == wanted,
                Provides::Kind(kind) => label.split_once(':').is_some_and(|(k, _)| k == kind),
            })
        })
}

pub(crate) fn run_search(
    terms: &[String],
    json: bool,
    paths: &StorePaths,
) -> Result<String, CliError> {
    let store = open_store(paths)?;
    let manifests = CacheManifests { paths };
    let index =
        Index::build(&store, &manifests).map_err(|error| CliError::Resolve(Box::new(error)))?;
    let query = Query::parse(terms);
    let results = search(entries(&index), &query);
    Ok(if json {
        format_json(&results, &query)
    } else {
        format_text(&results, &query)
    })
}

fn label_text((label, version): &(String, Option<String>)) -> String {
    match version {
        Some(version) => format!("{label} {version}"),
        None => label.clone(),
    }
}

fn format_text(results: &[Entry], query: &Query) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "results = {}", results.len());
    for entry in results {
        let _ = writeln!(out, "result:");
        let _ = writeln!(out, "  object = object:{}", entry.object);
        let _ = writeln!(out, "  name = {}", entry.name);
        if let Some(version) = &entry.version {
            let _ = writeln!(out, "  version = {version}");
        }
        if let Some(summary) = &entry.summary {
            let _ = writeln!(out, "  summary = {summary}");
        }
        if !entry.tags.is_empty() {
            let _ = writeln!(out, "  tags = {}", entry.tags.join(", "));
        }
        if !entry.provides.is_empty() {
            let labels: Vec<String> = entry.provides.iter().map(label_text).collect();
            let _ = writeln!(out, "  provides = {}", labels.join(", "));
        }
        if let Some(parent) = &entry.parent {
            let _ = writeln!(out, "  parent = object:{parent}");
        }
    }
    for term in &query.ignored {
        let _ = writeln!(out, "ignored = {term}");
    }
    // Search finds candidates; it doesn't verify them (SEARCH.md §2).
    let _ = writeln!(out, "source = local");
    let _ = writeln!(out, "validation = unverified");
    out
}

fn format_json(results: &[Entry], query: &Query) -> String {
    let results: Vec<serde_json::Value> = results
        .iter()
        .map(|entry| {
            serde_json::json!({
                "object_id": entry.object.to_string(),
                "snapshot_id": entry.snapshot,
                "name": entry.name,
                "summary": entry.summary,
                "tags": entry.tags,
                "provides": entry.provides.iter().map(|(label, version)| serde_json::json!({
                    "provides": label,
                    "version": version,
                })).collect::<Vec<_>>(),
                "version": entry.version,
                "parent": entry.parent.as_ref().map(ToString::to_string),
                "source": "local",
                "validation": "unverified",
                "locality": "local",
            })
        })
        .collect();
    format!(
        "{}\n",
        serde_json::to_string_pretty(&serde_json::json!({
            "results": results,
            "ignored": query.ignored,
        }))
        .unwrap_or_default()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, tags: &[&str], provides: &[&str], parent: bool) -> Entry {
        Entry {
            object: ObjectId::from_bytes(b"test", name.as_bytes()),
            snapshot: String::new(),
            name: name.to_owned(),
            summary: Some(format!("The {name} program.")),
            tags: tags.iter().map(|tag| (*tag).to_owned()).collect(),
            provides: provides
                .iter()
                .map(|label| ((*label).to_owned(), Some("1.0".to_owned())))
                .collect(),
            version: None,
            parent: parent.then(|| ObjectId::from_bytes(b"test", b"parent")),
        }
    }

    fn names(results: &[Entry]) -> Vec<&str> {
        results.iter().map(|entry| entry.name.as_str()).collect()
    }

    #[test]
    fn a_query_parses_words_filters_and_unknown_filters() {
        let query = Query::parse(&[
            "GNU tag:Shell".to_owned(),
            "provides:lib:*".to_owned(),
            "provides:tool:bash".to_owned(),
            "env:dosbox".to_owned(),
        ]);
        assert_eq!(
            query,
            Query {
                words: vec!["gnu".to_owned()],
                tags: vec!["shell".to_owned()],
                provides: vec![
                    Provides::Kind("lib".to_owned()),
                    Provides::Label("tool:bash".to_owned())
                ],
                ignored: vec!["env:dosbox".to_owned()],
            }
        );
    }

    #[test]
    fn words_and_filters_all_have_to_match() {
        let all = || {
            vec![
                entry("bash", &["shell", "gnu"], &["tool:bash"], true),
                entry("zlib", &["compression"], &["lib:zlib"], true),
                entry("zsh", &["shell"], &["tool:zsh"], false),
                entry("bash-doc", &["gnu"], &["doc:bash"], true),
            ]
        };
        let found = |terms: &str| {
            names(&search(all(), &Query::parse(&[terms.to_owned()])))
                .into_iter()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        };
        // An exact name first, then the rest by name.
        assert_eq!(found("bash"), ["bash", "bash-doc"]);
        assert_eq!(found("PROGRAM tag:shell"), ["bash", "zsh"]);
        assert_eq!(found("provides:lib:*"), ["zlib"]);
        assert_eq!(found("provides:tool:bash"), ["bash"]);
        assert_eq!(found("provides:doc:* gnu"), ["bash-doc"]);
        assert_eq!(found("unknown:filter").len(), 4);
        assert!(found("perl").is_empty());
    }
}
