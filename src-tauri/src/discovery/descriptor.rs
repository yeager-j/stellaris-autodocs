//! Minimal reader for `.mod` descriptor metadata. Advisory display data only:
//! discovery "reads only the metadata needed to populate the Mod Library"
//! (docs/technical-design.md, "Source module"). The real Clausewitz parser arrives in
//! Phase 4 and analysis never consumes this reader, so a tolerant line scanner is
//! proportionate here and a shared parser dependency is not.

/// Observed `.mod` fields (AGENTS.md, "Mod activation"). Everything optional; absence
/// and malformation are advisory facts, never scan failures. Declared Dependencies are
/// advisory metadata the Mod Library must display (PRD user story 10); they never
/// compose a Playset.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DescriptorMetadata {
    pub name: Option<String>,
    pub version: Option<String>,
    pub supported_version: Option<String>,
    pub tags: Vec<String>,
    pub dependencies: Vec<String>,
    pub remote_file_id: Option<String>,
}

pub fn parse_descriptor(text: &str) -> DescriptorMetadata {
    let mut metadata = DescriptorMetadata::default();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        if key == "tags" {
            metadata.tags = parse_string_list(value, &mut lines);
            continue;
        }
        if key == "dependencies" {
            metadata.dependencies = parse_string_list(value, &mut lines);
            continue;
        }
        let Some(unquoted) = unquote(value) else {
            continue;
        };
        match key {
            "name" => metadata.name = Some(unquoted),
            "version" => metadata.version = Some(unquoted),
            "supported_version" => metadata.supported_version = Some(unquoted),
            "remote_file_id" => metadata.remote_file_id = Some(unquoted),
            _ => {}
        }
    }
    metadata
}

/// `key={ "a" "b" }` on one line, or `key={` followed by one quoted entry per line
/// until `}` — both observed layouts (`tags`, `dependencies`).
fn parse_string_list<'a>(value: &str, lines: &mut impl Iterator<Item = &'a str>) -> Vec<String> {
    let Some(open) = value.strip_prefix('{') else {
        return Vec::new();
    };
    let mut tags = Vec::new();
    let mut collect = |segment: &str| {
        let mut rest = segment;
        while let Some(start) = rest.find('"') {
            let Some(len) = rest[start + 1..].find('"') else {
                return;
            };
            tags.push(rest[start + 1..start + 1 + len].to_owned());
            rest = &rest[start + len + 2..];
        }
    };
    if let Some(inline) = open.split('}').next().filter(|_| open.contains('}')) {
        collect(inline);
        return tags;
    }
    collect(open);
    for line in lines {
        if line.trim_start().starts_with('}') {
            break;
        }
        collect(line);
    }
    tags
}

fn unquote(value: &str) -> Option<String> {
    value
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_observed_descriptor_fields() {
        let text = r#"
name="Gigastructural Engineering & More"
version="3.44.*"
tags={
	"Technologies"
	"Gameplay"
}
supported_version="4.4.*"
remote_file_id="1121692237"
path="/some/absolute/path"
"#;
        let metadata = parse_descriptor(text);
        assert_eq!(
            metadata.name.as_deref(),
            Some("Gigastructural Engineering & More")
        );
        assert_eq!(metadata.version.as_deref(), Some("3.44.*"));
        assert_eq!(metadata.supported_version.as_deref(), Some("4.4.*"));
        assert_eq!(metadata.remote_file_id.as_deref(), Some("1121692237"));
        assert_eq!(metadata.tags, vec!["Technologies", "Gameplay"]);
    }

    #[test]
    fn tolerates_unknown_keys_malformed_lines_and_single_line_lists() {
        let text = "picture=\"thumb.png\"\nname=unquoted junk\ntags={ \"AI\" }\nname=\"Real\"";
        let metadata = parse_descriptor(text);
        // Malformed `name=unquoted junk` is skipped; the later valid line wins.
        assert_eq!(metadata.name.as_deref(), Some("Real"));
        assert_eq!(metadata.tags, vec!["AI"]);
    }

    #[test]
    fn reads_declared_dependencies() {
        let text = "name=\"Submod\"\ndependencies={\n\t\"Gigastructural Engineering & More\"\n\t\"UI Overhaul Dynamic\"\n}\n";
        let metadata = parse_descriptor(text);
        assert_eq!(
            metadata.dependencies,
            vec!["Gigastructural Engineering & More", "UI Overhaul Dynamic"]
        );
    }

    #[test]
    fn empty_input_is_empty_metadata() {
        assert_eq!(parse_descriptor(""), DescriptorMetadata::default());
    }
}
