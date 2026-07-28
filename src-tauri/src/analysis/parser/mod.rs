//! Private Paradox-script parsing seam.
//!
//! The rest of `analysis` consumes only the application-owned model re-exported here.
//! Parser-library values stop in the adapter, so replacing the parser changes this module
//! rather than every resolver and documentation consumer.

#[cfg(test)]
mod conformance;
mod digest;
mod jomini;
mod model;
#[cfg(test)]
mod ranges;

pub(super) use model::{
    Conditional, Container, EvidenceQuality, Field, Item, Operator, ParseFault, ParseFaultKind,
    ParsedFile, ParsedItem, Scalar, ScalarKind, SourceIdentity, SourceRange, Value,
};

use crate::canonical::encode::DigestBytes;

pub(super) fn parse(source: SourceIdentity, data: &[u8]) -> ParsedFile {
    jomini::parse(source, data)
}

pub(super) fn parsed_file_digest(file: &ParsedFile) -> DigestBytes {
    digest::parsed_file(file)
}

pub(super) fn parsed_corpus_digest<'a>(
    files: impl IntoIterator<Item = &'a ParsedFile>,
) -> DigestBytes {
    digest::corpus(files)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    #[test]
    fn model_sources_do_not_depend_on_the_parser_library() {
        let source_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("analysis")
            .join("parser");
        for name in ["model.rs", "digest.rs"] {
            let path = source_root.join(name);
            let source = fs::read_to_string(&path).unwrap();
            for (line_number, line) in source.lines().enumerate() {
                let is_comment = line.trim_start().starts_with("//");
                assert!(
                    is_comment || !line.to_ascii_lowercase().contains("jomini"),
                    "{}:{} names the parser library in code",
                    path.display(),
                    line_number + 1
                );
            }
        }

        let adapter = fs::read_to_string(source_root.join("jomini.rs")).unwrap();
        assert!(
            adapter.contains("use jomini::"),
            "negative control no longer detects the adapter dependency"
        );
    }
}
