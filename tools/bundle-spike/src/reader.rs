//! The Revision Reader: the sole owner of bundle I/O.
//!
//! `docs/technical-design.md:487` makes this boundary the reason the bundle format can be
//! chosen late. Opening validates the manifest and schema, and everything a caller can then
//! do is product-shaped: a documentation record, a browse index, revision issues, a decoded
//! search index, a localization table. There is no method that returns the bundle root, takes
//! a path, or reads an arbitrary file, because such a method would make every later claim
//! about this boundary a matter of discipline.
//!
//! It follows that a reader cannot address a staging directory or an unreferenced bundle:
//! [`Reader::open_published`] resolves a revision identifier through the published set, and
//! there is no other constructor.

use crate::bundle::{self, Manifest, Validation};
use crate::docmodel::{BrowseSummary, Entry, EntryKey};
use crate::resolve::Issue;
use crate::search::Index;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum OpenError {
    /// No published revision with this identifier.
    NotPublished(String),
    Invalid(Box<Validation>),
    Io(std::io::Error),
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OpenError::NotPublished(id) => write!(formatter, "no published revision {id}"),
            OpenError::Invalid(validation) => write!(formatter, "invalid bundle: {validation:?}"),
            OpenError::Io(error) => write!(formatter, "{error}"),
        }
    }
}

impl From<std::io::Error> for OpenError {
    fn from(error: std::io::Error) -> Self {
        OpenError::Io(error)
    }
}

pub struct Reader {
    /// Private, and there is no accessor. A caller that could reach this could reach anything
    /// under it.
    root: PathBuf,
    manifest: Manifest,
    /// Disposable in-memory caches. Eviction only causes a later reload from the immutable
    /// bundle (`docs/technical-design.md:653`), so nothing here is authoritative.
    indexes: BTreeMap<String, Index>,
    records: BTreeMap<String, Vec<Entry>>,
}

impl Reader {
    /// Open a published revision by identifier.
    ///
    /// The only constructor. A staging directory is named `.staging-…` and is not a revision
    /// identifier, so it is unreachable through this entry point by construction rather than
    /// by a check that could be forgotten — and the unreferenced-bundle case is handled the
    /// same way, by requiring the caller to present an identifier the published set contains.
    pub fn open_published(
        revisions_root: &Path,
        revision: &str,
        published: &[String],
    ) -> Result<Reader, OpenError> {
        if !published.iter().any(|candidate| candidate == revision) {
            return Err(OpenError::NotPublished(revision.to_owned()));
        }

        let root = bundle::published_path(revisions_root, revision);
        if !root.is_dir() {
            return Err(OpenError::NotPublished(revision.to_owned()));
        }

        let (manifest, validation) = bundle::validate(&root)?;
        if !validation.valid() {
            return Err(OpenError::Invalid(Box::new(validation)));
        }

        Ok(Reader {
            root,
            manifest,
            indexes: BTreeMap::new(),
            records: BTreeMap::new(),
        })
    }

    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// Read a documentation record.
    ///
    /// The lookup is layout-aware and the caller is not: an Entry Key goes in, an entry comes
    /// out, whether records are one per file or sharded. This is the whole point of the
    /// boundary — replacing bundle internals with SQLite changes this method and nothing
    /// above it.
    pub fn record(&mut self, key: &EntryKey) -> std::io::Result<Option<Entry>> {
        let slug = key.slug();
        let direct = format!("records/{slug}.json");
        if self.manifest.required_entries.contains_key(&direct) {
            let text = self.read(&direct)?;
            return Ok(Some(serde_json::from_str(&text)?));
        }

        for relative in self.manifest.required_entries.keys() {
            if !relative.starts_with("records/") {
                continue;
            }
            if !self.records.contains_key(relative) {
                let text = self.read(relative)?;
                let shard: Vec<Entry> = serde_json::from_str(&text)?;
                self.records.insert(relative.clone(), shard);
            }
            if let Some(found) = self.records[relative].iter().find(|entry| &entry.key == key) {
                return Ok(Some(found.clone()));
            }
        }
        Ok(None)
    }

    pub fn browse(&self, category: &str) -> std::io::Result<Vec<BrowseSummary>> {
        let text = self.read(&format!("browse/{category}.json"))?;
        Ok(serde_json::from_str(&text)?)
    }

    pub fn issues(&self) -> std::io::Result<Vec<Issue>> {
        let text = self.read("issues.json")?;
        Ok(serde_json::from_str(&text)?)
    }

    /// Load and decode a language's search index, retaining it.
    ///
    /// `search` owns the index contract and the algorithms; this owns physical loading and
    /// hands over a validated decoded index (`docs/technical-design.md:493`).
    pub fn search_index(&mut self, language: &str) -> std::io::Result<&Index> {
        if !self.indexes.contains_key(language) {
            let text = self.read(&format!("search/{language}.json"))?;
            let index = Index::decode(&text).map_err(std::io::Error::other)?;
            self.indexes.insert(language.to_owned(), index);
        }
        Ok(&self.indexes[language])
    }

    pub fn localization(&self, language: &str) -> std::io::Result<BTreeMap<String, String>> {
        let text = self.read(&format!("localization/{language}.json"))?;
        Ok(serde_json::from_str(&text)?)
    }

    /// Bytes retained by the caches this reader currently holds.
    pub fn retained_bytes(&self) -> u64 {
        let indexes: u64 = self.indexes.values().map(Index::retained_bytes).sum();
        let records: u64 = self
            .records
            .values()
            .map(|shard| {
                shard
                    .iter()
                    .map(|entry| crate::record::to_compact_json(entry).len() as u64)
                    .sum::<u64>()
            })
            .sum();
        indexes + records
    }

    pub fn evict(&mut self) {
        self.indexes.clear();
        self.records.clear();
    }

    /// The one place a bundle-relative path becomes a filesystem path.
    ///
    /// Private, and only ever called with a name the manifest already listed or a name this
    /// module composed. A caller cannot reach it, so it cannot be handed a `../`.
    fn read(&self, relative: &str) -> std::io::Result<String> {
        std::fs::read_to_string(self.root.join(relative))
    }
}
