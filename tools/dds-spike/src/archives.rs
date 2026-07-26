//! Enumerate the names inside a zip archive, without decompressing anything.
//!
//! Exists for one question: do the DLC archives ship textures? `docs/technical-design.md:285`
//! states that they may supply referenced visual assets, and the asset module's source of truth
//! depends on whether that is true at the pinned build. An unverified sentence in a design
//! document is exactly the kind of inherited claim this repository's spikes are meant to check.
//!
//! Only the central directory is read, so no decompressor and no dependency is needed. The
//! central directory holds every entry's name, which is the whole question.

use std::path::Path;

/// The end-of-central-directory signature, and the record's fixed length before any comment.
const EOCD_SIGNATURE: [u8; 4] = [0x50, 0x4b, 0x05, 0x06];
const EOCD_LENGTH: usize = 22;
/// The central-directory file-header signature.
const CENTRAL_SIGNATURE: [u8; 4] = [0x50, 0x4b, 0x01, 0x02];

fn u16_at(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes([
        *bytes.get(offset)?,
        *bytes.get(offset + 1)?,
    ]))
}

fn u32_at(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes([
        *bytes.get(offset)?,
        *bytes.get(offset + 1)?,
        *bytes.get(offset + 2)?,
        *bytes.get(offset + 3)?,
    ]))
}

/// Every entry name in the archive, or `None` if it is not a readable zip.
pub fn entry_names(path: &Path) -> Option<Vec<String>> {
    let bytes = std::fs::read(path).ok()?;

    // The end-of-central-directory record sits at the end, after a comment of unknown length.
    let start = bytes.len().checked_sub(EOCD_LENGTH)?;
    let eocd = (0..=start)
        .rev()
        .find(|offset| bytes[*offset..].starts_with(&EOCD_SIGNATURE))?;

    let count = u16_at(&bytes, eocd + 10)? as usize;
    let mut offset = u32_at(&bytes, eocd + 16)? as usize;

    let mut names = Vec::with_capacity(count);
    for _ in 0..count {
        if !bytes.get(offset..)?.starts_with(&CENTRAL_SIGNATURE) {
            break;
        }
        let name_length = u16_at(&bytes, offset + 28)? as usize;
        let extra_length = u16_at(&bytes, offset + 30)? as usize;
        let comment_length = u16_at(&bytes, offset + 32)? as usize;
        let name_start = offset + 46;
        let name = bytes.get(name_start..name_start + name_length)?;
        names.push(String::from_utf8_lossy(name).into_owned());
        offset = name_start + name_length + extra_length + comment_length;
    }
    Some(names)
}

/// One archive's contents, summarized by entry extension.
#[derive(Debug, Clone)]
pub struct ArchiveSummary {
    pub logical: String,
    pub entries: usize,
    pub extensions: std::collections::BTreeMap<String, usize>,
}

/// Summarize every `.zip` under `dlc/` in an installation root.
pub fn scan_dlc(install_root: &Path) -> Vec<ArchiveSummary> {
    let mut summaries = Vec::new();
    let dlc = install_root.join("dlc");
    let Ok(entries) = std::fs::read_dir(&dlc) else {
        return summaries;
    };
    let mut directories: Vec<_> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    directories.sort();

    for directory in directories {
        let Ok(files) = std::fs::read_dir(&directory) else {
            continue;
        };
        let mut archives: Vec<_> = files
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "zip"))
            .collect();
        archives.sort();

        for archive in archives {
            let Some(names) = entry_names(&archive) else {
                continue;
            };
            let mut extensions = std::collections::BTreeMap::new();
            for name in &names {
                let extension = Path::new(name)
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .map(|ext| ext.to_ascii_lowercase())
                    .unwrap_or_else(|| "(none)".into());
                *extensions.entry(extension).or_default() += 1;
            }
            summaries.push(ArchiveSummary {
                logical: crate::corpus::logical_path(install_root, &archive),
                entries: names.len(),
                extensions,
            });
        }
    }
    summaries
}
