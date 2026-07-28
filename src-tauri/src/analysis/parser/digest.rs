//! Canonical identity for the complete parsed evidence model.
//!
//! Unlike the spike's adapter-comparison digest, this includes source ranges, evidence
//! quality, and faults. Two parsed files that describe the same tokens at different source
//! locations are different evidence and should not share a digest.

use super::{
    Conditional, Container, Field, Item, ParseFault, ParsedFile, Scalar, SourceIdentity,
    SourceRange, Value,
};
use crate::canonical::encode::{CanonicalDigest, DigestBytes};
use crate::source::SourceKind;

const FILE_DOMAIN: &str = "stellaris-docs/parsed-file/v1";
const CORPUS_DOMAIN: &str = "stellaris-docs/parsed-corpus/v1";

pub(super) fn parsed_file(file: &ParsedFile) -> DigestBytes {
    let mut digest = CanonicalDigest::new(FILE_DOMAIN);
    write_source(&mut digest, file);
    write_items(&mut digest, &file.items);
    digest.begin_seq(file.faults.len());
    for fault in &file.faults {
        write_fault(&mut digest, fault);
    }
    digest.finish()
}

pub(super) fn corpus<'a>(files: impl IntoIterator<Item = &'a ParsedFile>) -> DigestBytes {
    corpus_of(
        files
            .into_iter()
            .map(|file| (file.source.clone(), parsed_file(file))),
    )
}

/// The same fold over per-file digests that have already been computed.
///
/// Exists because a whole-corpus run cannot hold every parsed tree at once: vanilla alone is
/// millions of scalars, each owning its exact bytes. Digesting a file and dropping its tree
/// keeps the run's memory bounded by the largest single file. The sort and the fold stay
/// here rather than being repeated by the caller, so both surfaces produce one corpus digest
/// by one rule.
pub(super) fn corpus_of(
    entries: impl IntoIterator<Item = (SourceIdentity, DigestBytes)>,
) -> DigestBytes {
    let mut entries: Vec<_> = entries.into_iter().collect();
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    let mut digest = CanonicalDigest::new(CORPUS_DOMAIN);
    digest.begin_seq(entries.len());
    for (_, file) in entries {
        digest.bytes(&file.0);
    }
    digest.finish()
}

fn write_source(digest: &mut CanonicalDigest, file: &ParsedFile) {
    let source = match file.source.source {
        SourceKind::VanillaContent => 1,
        SourceKind::TargetMod => 2,
    };
    digest.u64(source).text(file.source.logical.as_str());
}

fn write_items(digest: &mut CanonicalDigest, items: &[super::ParsedItem]) {
    digest.begin_seq(items.len());
    for parsed in items {
        digest.u64(parsed.evidence as u64);
        write_item(digest, &parsed.item);
    }
}

fn write_nested_items(digest: &mut CanonicalDigest, items: &[Item]) {
    digest.begin_seq(items.len());
    for item in items {
        write_item(digest, item);
    }
}

fn write_item(digest: &mut CanonicalDigest, item: &Item) {
    match item {
        Item::Field(field) => {
            digest.u64(1);
            write_field(digest, field);
        }
        Item::Element(value) => {
            digest.u64(2);
            write_value(digest, value);
        }
        Item::Conditional(conditional) => {
            digest.u64(3);
            write_conditional(digest, conditional);
        }
    }
}

fn write_field(digest: &mut CanonicalDigest, field: &Field) {
    write_range(digest, field.range);
    write_scalar(digest, &field.key);
    digest.u64(field.operator as u64);
    write_value(digest, &field.value);
}

fn write_conditional(digest: &mut CanonicalDigest, conditional: &Conditional) {
    write_range(digest, conditional.range);
    digest
        .bytes(&conditional.parameter)
        .bool(conditional.negated);
    write_nested_items(digest, &conditional.items);
}

fn write_value(digest: &mut CanonicalDigest, value: &Value) {
    match value {
        Value::Scalar(scalar) => {
            digest.u64(1);
            write_scalar(digest, scalar);
        }
        Value::Container(container) => {
            digest.u64(2);
            write_container(digest, container);
        }
        Value::Tagged {
            tag,
            container,
            range,
        } => {
            digest.u64(3);
            write_range(digest, *range);
            write_scalar(digest, tag);
            write_container(digest, container);
        }
    }
}

fn write_container(digest: &mut CanonicalDigest, container: &Container) {
    digest.u64(container.kind as u64);
    write_range(digest, container.range);
    write_nested_items(digest, &container.items);
}

fn write_scalar(digest: &mut CanonicalDigest, scalar: &Scalar) {
    digest.u64(scalar.kind as u64);
    write_range(digest, scalar.range);
    digest.bytes(&scalar.raw);
}

fn write_range(digest: &mut CanonicalDigest, range: SourceRange) {
    digest.u64(range.start).u64(range.end);
}

fn write_fault(digest: &mut CanonicalDigest, fault: &ParseFault) {
    digest.u64(fault.kind as u64).u64(fault.position);
    match fault.recovery_boundary {
        Some(boundary) => {
            digest.some().u64(boundary);
        }
        None => {
            digest.none();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::parser::{EvidenceQuality, Item, SourceIdentity, parse};
    use crate::canonical::path::LogicalPath;
    use crate::source::SourceKind;
    use std::fs;
    use std::path::{Path, PathBuf};

    fn identity(path: &str) -> SourceIdentity {
        SourceIdentity::new(SourceKind::TargetMod, LogicalPath::parse(path).unwrap())
    }

    #[test]
    fn source_ranges_participate_in_the_file_digest() {
        let mut file = parse(identity("shift.txt"), b"tech_x = { cost = 100 }\n");
        let before = parsed_file(&file);

        let Item::Field(field) = &mut file.items[0].item else {
            panic!("fixture begins with a field");
        };
        field.key.range.start += 1;

        assert_ne!(parsed_file(&file), before);
    }

    #[test]
    fn recovered_evidence_and_recovery_boundaries_participate_in_the_file_digest() {
        let source = b"first = { cost = 100 } }\nsecond = { cost = 200 }\n";
        let file = parse(identity("recovered.txt"), source);
        assert_eq!(file.faults.len(), 1);
        assert!(file.faults[0].recovery_boundary.is_some());
        assert_eq!(file.items[1].evidence, EvidenceQuality::Recovered);

        let expected = parsed_file(&file);
        assert_eq!(
            parsed_file(&parse(identity("recovered.txt"), source)),
            expected
        );

        let mut clean_evidence = file.clone();
        clean_evidence.items[1].evidence = EvidenceQuality::Clean;
        assert_ne!(parsed_file(&clean_evidence), expected);

        let mut no_boundary = file;
        no_boundary.faults[0].recovery_boundary = None;
        assert_ne!(parsed_file(&no_boundary), expected);
    }

    #[test]
    fn corpus_digest_is_independent_of_enumeration_and_parallel_scheduling() {
        let fixtures = valid_fixtures();
        let sorted: Vec<_> = fixtures
            .iter()
            .map(|(source, bytes)| parse(source.clone(), bytes))
            .collect();

        let mut reversed_input = fixtures.clone();
        reversed_input.reverse();
        let reversed: Vec<_> = reversed_input
            .iter()
            .map(|(source, bytes)| parse(source.clone(), bytes))
            .collect();

        let threads: Vec<_> = fixtures
            .into_iter()
            .map(|(source, bytes)| std::thread::spawn(move || parse(source, &bytes)))
            .collect();
        let parallel: Vec<_> = threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect();

        let expected = corpus(&sorted);
        assert_eq!(corpus(&reversed), expected);
        assert_eq!(corpus(&parallel), expected);
    }

    fn valid_fixtures() -> Vec<(SourceIdentity, Vec<u8>)> {
        let root = fixture_root().join("valid");
        let mut paths = Vec::new();
        collect_files(&root, &mut paths);
        paths.sort();
        paths
            .into_iter()
            .map(|path| {
                let relative = path.strip_prefix(&root).unwrap();
                let logical = relative
                    .components()
                    .map(|component| component.as_os_str().to_str().unwrap())
                    .collect::<Vec<_>>()
                    .join("/");
                (identity(&logical), fs::read(path).unwrap())
            })
            .collect()
    }

    fn collect_files(root: &Path, files: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(root).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_dir() {
                collect_files(&path, files);
            } else {
                files.push(path);
            }
        }
    }

    fn fixture_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("fixtures")
            .join("parser")
    }
}
