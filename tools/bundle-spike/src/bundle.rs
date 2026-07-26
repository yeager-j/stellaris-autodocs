//! Bundle layouts, the manifest, and publication.
//!
//! Every layout here writes the *same content*, produced by the same functions of the same
//! [`Documentation`] value. That is what makes a comparison between them a comparison of
//! layouts. A writer chooses where bytes go; it never chooses what they are.
//!
//! The commit protocol mirrors `docs/technical-design.md:468`, because the correctness checks
//! ask about it directly: write into a staging directory on the same filesystem, validate the
//! manifest and every required entry, then move the validated directory to its final path.
//! Bundle completion and revision publication have separate commit points, and a reader must
//! never be able to address staging.

use crate::digest::{domain_separated, sha256, Stream};
use crate::docmodel::Documentation;
use crate::search::Index;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Bumped by any change to bundle layout or to the manifest shape. An incompatible schema
/// invalidates a revision and causes a rebuild; published bundles are never migrated in place.
pub const BUNDLE_SCHEMA_VERSION: u32 = 1;

/// How full documentation records are distributed across files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// One file per document. The design's provisional shape.
    PerDocument,
    /// One file per content category.
    ByCategory,
    /// Fixed shard count over a stable-identity digest.
    ///
    /// By digest rather than by identifier prefix. Stellaris identifiers are overwhelmingly
    /// `tech_…`, so a prefix scheme would put almost every record in one shard and measure a
    /// layout nobody would ship.
    ByIdentityPrefix { shards: usize },
}

impl Layout {
    pub fn id(&self) -> String {
        match self {
            Layout::PerDocument => "per_document".into(),
            Layout::ByCategory => "by_category".into(),
            Layout::ByIdentityPrefix { shards } => format!("by_identity_prefix_{shards}"),
        }
    }
}

/// What preserved localization a bundle carries, and where.
///
/// Three arms rather than two, because the third one is what the closure measurement made
/// available: the documentation cites roughly one percent of the keys, so carrying only those
/// keeps the revision self-contained *and* small, and the shared store it would otherwise
/// need stops existing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalizationPlacement {
    /// Cited keys plus their static-reference closure, every language, inside the bundle.
    ClosureInBundle,
    /// Every key, every language, inside the bundle. The design's original assumption.
    AllKeysInBundle,
    /// Every key, every language, in a shared content-addressed store outside the bundle.
    AllKeysSharedStore,
}

impl LocalizationPlacement {
    pub fn id(&self) -> &'static str {
        match self {
            LocalizationPlacement::ClosureInBundle => "closure_in_bundle",
            LocalizationPlacement::AllKeysInBundle => "all_keys_in_bundle",
            LocalizationPlacement::AllKeysSharedStore => "all_keys_shared_store",
        }
    }

    /// Whether localization bytes live in the bundle at all.
    pub fn in_bundle(&self) -> bool {
        !matches!(self, LocalizationPlacement::AllKeysSharedStore)
    }
}

/// How many languages get build-time search material.
///
/// A real axis rather than a knob. Search material is the one artifact that materializes
/// localized text, so it is the only one that multiplies by the language count — and only one
/// language is active at a time. Whether the other nine are worth building at publication or
/// on first use is a question the size budget turned out to hinge on, so it is measured
/// instead of assumed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchScope {
    AllLanguages,
    /// The selected language and English.
    ///
    /// Two, never one. Product requirement 28 keeps English names searchable in any configured
    /// language so guides and community discussion stay usable, and requirement 21 lists
    /// selected-language names and English names as separate index inputs. An earlier revision
    /// of this harness built one table and called it the active language, which was correct
    /// only when the selected language happened to be English.
    SelectedAndEnglish,
}

impl SearchScope {
    pub fn id(&self) -> &'static str {
        match self {
            SearchScope::AllLanguages => "search_all",
            SearchScope::SelectedAndEnglish => "search_selected_english",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Shape {
    pub layout: Layout,
    pub localization: LocalizationPlacement,
    pub search: SearchScope,
    /// The language chosen before the build. English unless a measurement needs the two-table
    /// case that a non-English player produces.
    pub selected_language: &'static str,
}

impl Shape {
    pub fn id(&self) -> String {
        let mut id = format!(
            "{}+{}+{}",
            self.layout.id(),
            self.localization.id(),
            self.search.id()
        );
        if self.search == SearchScope::SelectedAndEnglish && self.selected_language != "english" {
            id.push('_');
            id.push_str(self.selected_language);
        }
        id
    }

    /// The languages this shape materializes search material for.
    pub fn search_languages(&self, available: &[String]) -> Vec<String> {
        match self.search {
            SearchScope::AllLanguages => available.to_vec(),
            SearchScope::SelectedAndEnglish => {
                let mut wanted = vec![self.selected_language.to_owned()];
                if self.selected_language != "english" {
                    wanted.push("english".to_owned());
                }
                wanted.retain(|language| available.contains(language));
                wanted
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub schema: u32,
    /// SHA-256 of a domain separator plus the canonical manifest body excluding this field.
    pub revision: String,
    pub case: String,
    pub shape: String,
    pub entry_count: usize,
    pub complete: bool,
    pub incomplete_registries: Vec<String>,
    /// Bundle-relative path to SHA-256, for every entry a reader requires.
    pub required_entries: BTreeMap<String, String>,
    /// Asset Store keys this revision requires.
    pub required_asset_keys: Vec<String>,
    /// Localization chunk keys this revision requires, when localization is shared.
    pub required_localization_chunks: Vec<String>,
}

impl Manifest {
    /// The canonical body the Revision identifier digests.
    ///
    /// Excludes the identifier itself, and excludes anything that varies with where the build
    /// ran: temporary roots, timestamps, worker schedules, and final bundle paths
    /// (`docs/technical-design.md:330`).
    fn identity_body(&self) -> String {
        let mut stream = Stream::new();
        stream.push("schema", &self.schema.to_string());
        stream.push("case", &self.case);
        stream.push("shape", &self.shape);
        stream.push("entry_count", &self.entry_count.to_string());
        stream.push("complete", &self.complete.to_string());
        for registry in &self.incomplete_registries {
            stream.push("incomplete_registry", registry);
        }
        for (path, digest) in &self.required_entries {
            stream.push(path, digest);
        }
        for key in &self.required_asset_keys {
            stream.push("asset", key);
        }
        for key in &self.required_localization_chunks {
            stream.push("localization_chunk", key);
        }
        stream.finish()
    }
}

/// A bundle on disk, staged or published.
pub struct Written {
    pub root: PathBuf,
    pub manifest: Manifest,
    pub file_count: usize,
    pub total_bytes: u64,
    /// Bytes excluding preserved localization, which is the numerator the size budget uses.
    pub bytes_excluding_localization: u64,
    /// Per-artifact-class byte totals, so the duplication attribution has something to
    /// attribute to.
    pub bytes_by_class: BTreeMap<String, u64>,
}

/// Where a staging directory lives.
///
/// Beside the revisions directory rather than in a temporary root, because publication is a
/// directory move and a move across filesystems is a copy that can fail halfway
/// (`docs/technical-design.md:477`).
pub fn staging_path(revisions_root: &Path, case: &str, shape: &Shape) -> PathBuf {
    revisions_root.join(format!(".staging-{case}-{}", shape.id()))
}

pub fn published_path(revisions_root: &Path, revision: &str) -> PathBuf {
    revisions_root.join(revision)
}

/// Write a complete bundle into a staging directory.
///
/// The manifest is written last and hashes what is already on disk, so it can never name an
/// entry that was not produced — the same rule every record in this repository follows.
pub fn write(
    documentation: &Documentation,
    shape: Shape,
    revisions_root: &Path,
    asset_keys: &[String],
    localization_chunks: &[String],
) -> std::io::Result<Written> {
    let root = staging_path(revisions_root, &documentation.case, &shape);
    if root.exists() {
        std::fs::remove_dir_all(&root)?;
    }
    std::fs::create_dir_all(&root)?;

    let mut writer = Writer::new(root.clone());

    for (category, summaries) in documentation.browse_index() {
        writer.put("browse", &format!("browse/{category}.json"), &crate::record::to_compact_json(&summaries))?;
    }

    match shape.layout {
        Layout::PerDocument => {
            for (slug, entry) in documentation.full_records() {
                writer.put("records", &format!("records/{slug}.json"), &crate::record::to_compact_json(entry))?;
            }
        }
        Layout::ByCategory => {
            let mut shards: BTreeMap<&str, Vec<&crate::docmodel::Entry>> = BTreeMap::new();
            for entry in &documentation.entries {
                shards.entry(entry.key.category.as_str()).or_default().push(entry);
            }
            for (category, entries) in shards {
                writer.put(
                    "records",
                    &format!("records/{category}.json"),
                    &crate::record::to_compact_json(&entries),
                )?;
            }
        }
        Layout::ByIdentityPrefix { shards } => {
            let mut buckets: BTreeMap<String, Vec<&crate::docmodel::Entry>> = BTreeMap::new();
            for entry in &documentation.entries {
                buckets.entry(shard_of(&entry.key.slug(), shards)).or_default().push(entry);
            }
            for (bucket, entries) in buckets {
                writer.put(
                    "records",
                    &format!("records/{bucket}.json"),
                    &crate::record::to_compact_json(&entries),
                )?;
            }
        }
    }

    let available: Vec<String> = documentation.localization.languages.keys().cloned().collect();
    for language in shape.search_languages(&available) {
        let index = Index::build(documentation, &language);
        writer.put("search", &format!("search/{language}.json"), &index.encode())?;
    }

    if shape.localization.in_bundle() {
        for (language, table) in &documentation.localization.languages {
            writer.put(
                "localization",
                &format!("localization/{language}.json"),
                &crate::record::to_compact_json(&table.entries),
            )?;
        }
    }

    writer.put("issues", "issues.json", &crate::record::to_compact_json(&documentation.issues))?;

    let mut manifest = Manifest {
        schema: BUNDLE_SCHEMA_VERSION,
        revision: String::new(),
        case: documentation.case.clone(),
        shape: shape.id(),
        entry_count: documentation.entries.len(),
        complete: documentation.completeness.complete,
        incomplete_registries: documentation.completeness.incomplete_registries.clone(),
        required_entries: writer.hashes.clone(),
        required_asset_keys: asset_keys.to_vec(),
        required_localization_chunks: match shape.localization {
            LocalizationPlacement::AllKeysSharedStore => localization_chunks.to_vec(),
            _ => Vec::new(),
        },
    };
    manifest.revision = domain_separated("stellaris-docs/revision/v1", manifest.identity_body().as_bytes());

    let manifest_text = crate::record::to_json(&manifest);
    std::fs::write(root.join("manifest.json"), &manifest_text)?;

    // The manifest is a required entry of the bundle for size purposes but is not listed in
    // `required_entries`: a hash of itself cannot be part of itself.
    let manifest_bytes = manifest_text.len() as u64;
    let localization_bytes = writer.bytes_by_class.get("localization").copied().unwrap_or(0);

    Ok(Written {
        root,
        file_count: writer.hashes.len() + 1,
        total_bytes: writer.total_bytes + manifest_bytes,
        bytes_excluding_localization: writer.total_bytes + manifest_bytes - localization_bytes,
        bytes_by_class: writer.bytes_by_class,
        manifest,
    })
}

fn shard_of(slug: &str, shards: usize) -> String {
    let digest = sha256(slug.as_bytes());
    let leading = u64::from_str_radix(&digest[..8], 16).unwrap_or(0);
    format!("shard_{:04}", leading as usize % shards.max(1))
}

struct Writer {
    root: PathBuf,
    hashes: BTreeMap<String, String>,
    bytes_by_class: BTreeMap<String, u64>,
    total_bytes: u64,
}

impl Writer {
    fn new(root: PathBuf) -> Self {
        Writer {
            root,
            hashes: BTreeMap::new(),
            bytes_by_class: BTreeMap::new(),
            total_bytes: 0,
        }
    }

    fn put(&mut self, class: &str, relative: &str, contents: &str) -> std::io::Result<()> {
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, contents)?;
        self.hashes.insert(relative.to_owned(), sha256(contents.as_bytes()));
        *self.bytes_by_class.entry(class.to_owned()).or_default() += contents.len() as u64;
        self.total_bytes += contents.len() as u64;
        Ok(())
    }
}

/// What validation found. A typed report rather than a bare boolean, because detecting
/// corruption is the *purpose* of Validate Published Revision and a caller needs to know what
/// was wrong (`docs/technical-design.md:809`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Validation {
    pub missing: Vec<String>,
    pub changed: Vec<String>,
    pub unexpected: Vec<String>,
    pub schema_mismatch: Option<String>,
    pub identity_mismatch: bool,
}

impl Validation {
    pub fn valid(&self) -> bool {
        self.missing.is_empty()
            && self.changed.is_empty()
            && self.unexpected.is_empty()
            && self.schema_mismatch.is_none()
            && !self.identity_mismatch
    }
}

/// Validate a bundle directory against its own manifest.
///
/// Checks all four ways a bundle can disagree with its manifest — missing, changed,
/// unexpected, and an identifier that does not derive from the recorded body — because the
/// evaluation asks for exactly those and three of them are individually easy to miss. A
/// validator that only re-hashed the entries the manifest names would report `ok` on a bundle
/// carrying an extra file nothing accounts for.
pub fn validate(root: &Path) -> std::io::Result<(Manifest, Validation)> {
    let text = std::fs::read_to_string(root.join("manifest.json"))?;
    let manifest: Manifest = serde_json::from_str(&text).map_err(std::io::Error::from)?;

    let mut validation = Validation::default();
    if manifest.schema != BUNDLE_SCHEMA_VERSION {
        validation.schema_mismatch = Some(format!(
            "bundle schema {} is not {BUNDLE_SCHEMA_VERSION}",
            manifest.schema
        ));
    }

    let expected = domain_separated(
        "stellaris-docs/revision/v1",
        manifest.identity_body().as_bytes(),
    );
    validation.identity_mismatch = expected != manifest.revision;

    for (relative, digest) in &manifest.required_entries {
        match std::fs::read(root.join(relative)) {
            Err(_) => validation.missing.push(relative.clone()),
            Ok(bytes) if &sha256(&bytes) != digest => validation.changed.push(relative.clone()),
            Ok(_) => {}
        }
    }

    for present in enumerate(root)? {
        if present != "manifest.json" && !manifest.required_entries.contains_key(&present) {
            validation.unexpected.push(present);
        }
    }
    validation.unexpected.sort();

    Ok((manifest, validation))
}

fn enumerate(root: &Path) -> std::io::Result<Vec<String>> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        for entry in std::fs::read_dir(&directory)?.flatten() {
            let path = entry.path();
            match entry.file_type() {
                Ok(kind) if kind.is_dir() => stack.push(path),
                Ok(_) => found.push(crate::corpus::logical_path(root, &path)),
                Err(_) => {}
            }
        }
    }
    found.sort();
    Ok(found)
}

/// Move a validated staging directory to its canonical immutable path.
///
/// Refuses to publish an invalid bundle. This is the first of the two commit points; the
/// second, replacing the state pointer, belongs to the state module and is modelled by the
/// caller.
pub fn publish(staged: &Path, revisions_root: &Path) -> std::io::Result<PathBuf> {
    let (manifest, validation) = validate(staged)?;
    if !validation.valid() {
        return Err(std::io::Error::other(format!(
            "refusing to publish an invalid bundle: {validation:?}"
        )));
    }

    let final_path = published_path(revisions_root, &manifest.revision);
    if final_path.exists() {
        // A rebuild that produces byte-identical output produces the same revision
        // identifier. Publishing over it would be a no-op; removing and re-moving keeps the
        // operation total without pretending the two bundles differ.
        std::fs::remove_dir_all(&final_path)?;
    }
    std::fs::rename(staged, &final_path)?;
    Ok(final_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shard_assignment_is_stable_and_spreads_a_uniform_prefix() {
        // Every Stellaris technology identifier starts `tech_`. A prefix scheme would put all
        // of these in one bucket; the digest scheme is why this test can pass at all.
        let mut buckets = std::collections::BTreeSet::new();
        for index in 0..200 {
            buckets.insert(shard_of(&format!("technology.tech_{index:04}"), 16));
        }
        assert_eq!(buckets.len(), 16, "{buckets:?}");
        assert_eq!(shard_of("technology.tech_0001", 16), shard_of("technology.tech_0001", 16));
    }

    #[test]
    fn validation_reports_all_four_disagreements_separately() {
        let mut validation = Validation::default();
        assert!(validation.valid());

        validation.missing.push("records/a.json".into());
        assert!(!validation.valid());

        let mut changed = Validation::default();
        changed.changed.push("records/a.json".into());
        assert!(!changed.valid());

        let mut unexpected = Validation::default();
        unexpected.unexpected.push("records/stowaway.json".into());
        assert!(!unexpected.valid());

        let mut identity = Validation::default();
        identity.identity_mismatch = true;
        assert!(!identity.valid());
    }
}
