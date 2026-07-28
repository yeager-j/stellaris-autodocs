//! Adapts the incremental lexer into the application-owned parsed representation.
//!
//! A token's reader position after a read is not a reliable end boundary: unquoted tokens
//! may consume one trailing delimiter. The adapter instead records the position before the
//! read, skips the same trivia Clausewitz skips, and adds the token's known width.

use super::{
    Conditional, Container, EvidenceQuality, Field, Item, Operator, ParseFault, ParseFaultKind,
    ParsedFile, ParsedItem, Scalar, ScalarKind, SourceIdentity, SourceRange, Value,
};
use jomini::text::{Operator as JominiOperator, ReaderErrorKind, Token, TokenReader};

const MAX_DEPTH: usize = 256;

pub(super) fn parse(source: SourceIdentity, data: &[u8]) -> ParsedFile {
    parse_with_boundary_finder(source, data, resync)
}

fn parse_with_boundary_finder(
    source: SourceIdentity,
    data: &[u8],
    boundary_after: fn(&[u8], usize) -> Option<usize>,
) -> ParsedFile {
    let mut items = Vec::new();
    let mut faults = Vec::new();
    let mut base = 0usize;
    let mut evidence = EvidenceQuality::Clean;

    loop {
        let mut cursor = Cursor::new(&data[base..], base);
        match parse_items(&mut cursor, 0, Until::EndOfFile) {
            Ok(parsed) => {
                items.extend(with_evidence(parsed, evidence));
                break;
            }
            Err(stop) => {
                items.extend(with_evidence(stop.items, evidence));
                let recovery_boundary =
                    boundary_after(data, stop.error.position).filter(|boundary| *boundary > base);
                faults.push(ParseFault {
                    kind: stop.error.kind,
                    position: stop.error.position as u64,
                    recovery_boundary: recovery_boundary.map(|boundary| boundary as u64),
                });
                match recovery_boundary {
                    Some(boundary) => {
                        base = boundary;
                        evidence = EvidenceQuality::Recovered;
                    }
                    None => break,
                }
            }
        }
    }

    ParsedFile {
        source,
        items,
        faults,
    }
}

fn with_evidence(items: Vec<Item>, evidence: EvidenceQuality) -> Vec<ParsedItem> {
    items
        .into_iter()
        .map(|item| ParsedItem { evidence, item })
        .collect()
}

/// Resume at the next column-zero identifier line after the fault.
///
/// This is a heuristic about Stellaris source layout, not grammar. Around a fault it can
/// promote nested content to top level, so every item emitted after a restart is Recovered
/// rather than Clean. It cannot detect stray tokens that leave the source valid-looking.
fn resync(data: &[u8], after: usize) -> Option<usize> {
    let mut index = after.min(data.len());

    while index < data.len() && data[index] != b'\n' {
        index += 1;
    }

    while index < data.len() {
        index += 1;
        match data.get(index) {
            None => return None,
            Some(&first)
                if first.is_ascii_alphabetic()
                    || first == b'_'
                    || first == b'@'
                    || first == b'"' =>
            {
                return Some(index);
            }
            Some(_) => {
                while index < data.len() && data[index] != b'\n' {
                    index += 1;
                }
            }
        }
    }

    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AdapterError {
    kind: ParseFaultKind,
    position: usize,
    eof: bool,
}

struct Stop {
    error: AdapterError,
    items: Vec<Item>,
}

impl Stop {
    fn bare(error: AdapterError) -> Self {
        Self {
            error,
            items: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Until {
    EndOfFile,
    CloseBrace,
    CloseBracket,
}

fn parse_items(cursor: &mut Cursor<'_>, depth: usize, until: Until) -> Result<Vec<Item>, Stop> {
    let mut items = Vec::new();

    macro_rules! fail {
        ($error:expr) => {
            return Err(Stop {
                error: $error,
                items,
            })
        };
    }

    loop {
        cursor.set_conditional_scope(until == Until::CloseBracket);
        let lexeme = match cursor.advance() {
            Ok(Some(lexeme)) => lexeme,
            Ok(None) if until == Until::EndOfFile => return Ok(items),
            Ok(None) if until == Until::CloseBrace => {
                fail!(cursor.eof(ParseFaultKind::ContainerUnclosed))
            }
            Ok(None) => fail!(cursor.eof(ParseFaultKind::ConditionalUnclosed)),
            Err(error) => fail!(error),
        };

        match lexeme.lex {
            Lex::Close if until == Until::CloseBrace => return Ok(items),
            Lex::Close => fail!(at(lexeme.range, ParseFaultKind::UnexpectedCloseBrace)),
            Lex::Operator(_) => fail!(at(lexeme.range, ParseFaultKind::OperatorWithoutKey)),
            Lex::ConditionalClose if until == Until::CloseBracket => return Ok(items),
            Lex::ConditionalClose => fail!(at(
                lexeme.range,
                ParseFaultKind::ConditionalDelimiterWithoutBlock
            )),
            Lex::ConditionalOpen { parameter, negated } => {
                let body = match parse_items(cursor, depth + 1, Until::CloseBracket) {
                    Ok(body) => body,
                    Err(mut stop) => {
                        if stop.error.eof {
                            stop.error.kind = ParseFaultKind::ConditionalUnclosed;
                            stop.error.position = lexeme.range.start as usize;
                            stop.error.eof = false;
                        }
                        fail!(stop.error)
                    }
                };
                items.push(Item::Conditional(Conditional {
                    parameter,
                    negated,
                    items: body,
                    range: SourceRange::new(lexeme.range.start as usize, cursor.position()),
                }));
            }
            Lex::Open => {
                let container = match container(cursor, lexeme.range, depth) {
                    Ok(container) => container,
                    Err(stop) => fail!(stop.error),
                };
                items.push(Item::Element(Value::Container(container)));
            }
            Lex::Scalar(scalar) => {
                let operator = match cursor.peek() {
                    Ok(Some(Lexeme {
                        lex: Lex::Operator(operator),
                        ..
                    })) => Some((*operator, true)),
                    Ok(Some(Lexeme { lex: Lex::Open, .. })) => Some((Operator::Equal, false)),
                    Ok(_) => None,
                    Err(error) => fail!(error),
                };

                let Some((operator, explicit)) = operator else {
                    items.push(Item::Element(Value::Scalar(
                        scalar.into_model(lexeme.range),
                    )));
                    continue;
                };

                if explicit {
                    cursor.drop_peeked();
                }
                let key = scalar.into_model(lexeme.range);
                let value = match value(cursor, depth) {
                    Ok(value) => value,
                    Err(stop) => fail!(stop.error),
                };
                let range = SourceRange::new(key.range.start as usize, value.range().end as usize);
                items.push(Item::Field(Field {
                    key,
                    operator,
                    value,
                    range,
                }));
            }
        }
    }
}

fn value(cursor: &mut Cursor<'_>, depth: usize) -> Result<Value, Stop> {
    let lexeme = match cursor.advance() {
        Ok(Some(lexeme)) => lexeme,
        Ok(None) => {
            return Err(Stop::bare(cursor.eof(ParseFaultKind::ValueExpected)));
        }
        Err(error) => return Err(Stop::bare(error)),
    };

    match lexeme.lex {
        Lex::Open => Ok(Value::Container(container(cursor, lexeme.range, depth)?)),
        Lex::Scalar(scalar) => {
            let tagged = match cursor.peek() {
                Ok(Some(Lexeme { lex: Lex::Open, .. })) => true,
                Ok(_) => false,
                Err(error) => return Err(Stop::bare(error)),
            };
            if !tagged {
                return Ok(Value::Scalar(scalar.into_model(lexeme.range)));
            }

            let open = cursor.take_peeked();
            let container = container(cursor, open.range, depth)?;
            let range = SourceRange::new(lexeme.range.start as usize, container.range.end as usize);
            Ok(Value::Tagged {
                tag: scalar.into_model(lexeme.range),
                container,
                range,
            })
        }
        Lex::Close | Lex::Operator(_) | Lex::ConditionalOpen { .. } | Lex::ConditionalClose => {
            Err(Stop::bare(at(lexeme.range, ParseFaultKind::ValueExpected)))
        }
    }
}

fn container(cursor: &mut Cursor<'_>, open: SourceRange, depth: usize) -> Result<Container, Stop> {
    if depth >= MAX_DEPTH {
        return Err(Stop::bare(at(open, ParseFaultKind::NestingLimitExceeded)));
    }

    let items = match parse_items(cursor, depth + 1, Until::CloseBrace) {
        Ok(items) => items,
        Err(mut stop) => {
            stop.items.clear();
            if stop.error.eof {
                stop.error.kind = ParseFaultKind::ContainerUnclosed;
                stop.error.position = open.start as usize;
                stop.error.eof = false;
            }
            return Err(stop);
        }
    };
    let range = SourceRange::new(open.start as usize, cursor.position());
    Ok(Container::from_items(items, range))
}

fn at(range: SourceRange, kind: ParseFaultKind) -> AdapterError {
    AdapterError {
        kind,
        position: range.start as usize,
        eof: false,
    }
}

#[derive(Clone)]
struct RawScalar {
    kind: ScalarKind,
    raw: Vec<u8>,
}

impl RawScalar {
    fn into_model(self, range: SourceRange) -> Scalar {
        Scalar {
            kind: self.kind,
            raw: self.raw,
            range,
        }
    }
}

#[derive(Clone)]
enum Lex {
    Open,
    Close,
    Operator(Operator),
    Scalar(RawScalar),
    ConditionalOpen { parameter: Vec<u8>, negated: bool },
    ConditionalClose,
}

fn stellaris_construct(data: &[u8], at: usize, inside_conditional: bool) -> Option<(Lex, usize)> {
    match data.get(at)? {
        b'[' if data.get(at + 1) != Some(&b'[') => {
            let close = at + data[at..].iter().position(|byte| *byte == b']')?;
            Some((
                Lex::Scalar(RawScalar {
                    kind: ScalarKind::ScopeToken,
                    raw: data[at..=close].to_vec(),
                }),
                close + 1 - at,
            ))
        }
        b'[' if data.get(at + 1) == Some(&b'[') => {
            let negated = data.get(at + 2) == Some(&b'!');
            let name_from = at + if negated { 3 } else { 2 };
            let name_to = name_from + data[name_from..].iter().position(|byte| *byte == b']')?;
            Some((
                Lex::ConditionalOpen {
                    parameter: data[name_from..name_to].to_vec(),
                    negated,
                },
                name_to + 1 - at,
            ))
        }
        b']' if inside_conditional => Some((Lex::ConditionalClose, 1)),
        b'@' if data.get(at + 1) == Some(&b'\\') && data.get(at + 2) == Some(&b'[') => {
            let close = at + data[at..].iter().position(|byte| *byte == b']')?;
            Some((
                Lex::Scalar(RawScalar {
                    kind: ScalarKind::VariableExpr,
                    raw: data[at..=close].to_vec(),
                }),
                close + 1 - at,
            ))
        }
        _ => None,
    }
}

#[derive(Clone)]
struct Lexeme {
    lex: Lex,
    range: SourceRange,
}

struct Cursor<'a> {
    reader: TokenReader<'a>,
    data: &'a [u8],
    peeked: Option<Lexeme>,
    base: usize,
    last: usize,
    inside_conditional: bool,
}

impl<'a> Cursor<'a> {
    fn new(data: &'a [u8], base: usize) -> Self {
        Self {
            reader: TokenReader::from_slice(data),
            data,
            peeked: None,
            base,
            last: base,
            inside_conditional: false,
        }
    }

    fn set_conditional_scope(&mut self, inside: bool) {
        if self.inside_conditional == inside {
            return;
        }
        self.inside_conditional = inside;
        let bracket = matches!(
            &self.peeked,
            Some(Lexeme {
                lex: Lex::ConditionalClose,
                ..
            })
        ) || matches!(
            &self.peeked,
            Some(Lexeme {
                lex: Lex::Scalar(scalar),
                ..
            }) if scalar.raw == b"]"
        );
        if bracket {
            self.peeked = None;
        }
    }

    fn position(&self) -> usize {
        self.last
    }

    fn eof(&self, kind: ParseFaultKind) -> AdapterError {
        AdapterError {
            kind,
            position: self.base + self.reader.position(),
            eof: true,
        }
    }

    fn peek(&mut self) -> Result<Option<&Lexeme>, AdapterError> {
        if self.peeked.is_none() {
            self.peeked = self.read()?;
        }
        Ok(self.peeked.as_ref())
    }

    fn take_peeked(&mut self) -> Lexeme {
        let lexeme = self
            .peeked
            .take()
            .expect("take_peeked follows successful lookahead");
        self.last = lexeme.range.end as usize;
        lexeme
    }

    fn drop_peeked(&mut self) {
        let _ = self.take_peeked();
    }

    fn advance(&mut self) -> Result<Option<Lexeme>, AdapterError> {
        if self.peeked.is_some() {
            return Ok(Some(self.take_peeked()));
        }
        let lexeme = self.read()?;
        if let Some(lexeme) = &lexeme {
            self.last = lexeme.range.end as usize;
        }
        Ok(lexeme)
    }

    fn read(&mut self) -> Result<Option<Lexeme>, AdapterError> {
        let opens_at = skip_trivia(self.data, self.reader.position());

        if let Some((lex, width)) =
            stellaris_construct(self.data, opens_at, self.inside_conditional)
        {
            let consume = opens_at - self.reader.position() + width;
            self.reader
                .read_bytes(consume)
                .map_err(|error| reader_error(error, self.base))?;
            let start = self.base + opens_at;
            return Ok(Some(Lexeme {
                lex,
                range: SourceRange::new(start, start + width),
            }));
        }

        let (lex, width) = {
            let token = match self.reader.next() {
                Ok(Some(token)) => token,
                Ok(None) => return Ok(None),
                Err(error) => return Err(reader_error(error, self.base)),
            };
            match token {
                Token::Open => (Lex::Open, 1),
                Token::Close => (Lex::Close, 1),
                Token::Operator(operator) => (
                    Lex::Operator(convert_operator(operator)),
                    operator.symbol().len(),
                ),
                Token::Unquoted(scalar) => {
                    let raw = scalar.as_bytes().to_vec();
                    let width = raw.len();
                    (
                        Lex::Scalar(RawScalar {
                            kind: classify(&raw),
                            raw,
                        }),
                        width,
                    )
                }
                Token::Quoted(scalar) => {
                    let raw = scalar.as_bytes().to_vec();
                    let width = raw.len() + 2;
                    (
                        Lex::Scalar(RawScalar {
                            kind: ScalarKind::Quoted,
                            raw,
                        }),
                        width,
                    )
                }
            }
        };

        let start = self.base + opens_at;
        Ok(Some(Lexeme {
            lex,
            range: SourceRange::new(start, start + width),
        }))
    }
}

fn reader_error(error: jomini::ReaderError, base: usize) -> AdapterError {
    AdapterError {
        kind: ParseFaultKind::TokenReaderFailure,
        position: base + error.position(),
        eof: matches!(error.kind(), ReaderErrorKind::Eof),
    }
}

fn skip_trivia(data: &[u8], from: usize) -> usize {
    let mut index = from;
    if index == 0 && data.starts_with(&[0xEF, 0xBB, 0xBF]) {
        index = 3;
    }
    loop {
        while index < data.len() && (data[index].is_ascii_whitespace() || data[index] == b';') {
            index += 1;
        }
        if data.get(index) != Some(&b'#') {
            return index;
        }
        while index < data.len() && data[index] != b'\n' {
            index += 1;
        }
    }
}

fn convert_operator(operator: JominiOperator) -> Operator {
    match operator {
        JominiOperator::Equal => Operator::Equal,
        JominiOperator::Exact => Operator::Exact,
        JominiOperator::NotEqual => Operator::NotEqual,
        JominiOperator::LessThan => Operator::LessThan,
        JominiOperator::LessThanEqual => Operator::LessThanEqual,
        JominiOperator::GreaterThan => Operator::GreaterThan,
        JominiOperator::GreaterThanEqual => Operator::GreaterThanEqual,
        JominiOperator::Exists => Operator::Exists,
    }
}

fn classify(raw: &[u8]) -> ScalarKind {
    match raw.first() {
        Some(b'@') if raw.get(1) == Some(&b'[') => ScalarKind::VariableExpr,
        Some(b'@') if raw.len() > 1 => ScalarKind::VariableRef,
        Some(b'$') if raw.len() > 1 && raw.last() == Some(&b'$') => ScalarKind::Parameter,
        _ if is_number(raw) => ScalarKind::Number,
        _ => ScalarKind::Unquoted,
    }
}

fn is_number(raw: &[u8]) -> bool {
    let body = match raw.first() {
        Some(b'-') | Some(b'+') => &raw[1..],
        _ => raw,
    };
    if body.is_empty() {
        return false;
    }
    let mut digits = 0usize;
    let mut dots = 0usize;
    for byte in body {
        match byte {
            b'0'..=b'9' => digits += 1,
            b'.' => dots += 1,
            _ => return false,
        }
    }
    digits > 0 && dots <= 1
}

#[cfg(test)]
mod tests {
    use super::super::ranges::verify_ranges;
    use super::*;
    use crate::canonical::path::LogicalPath;
    use crate::source::SourceKind;
    use proptest::prelude::*;
    use std::fs;
    use std::path::{Path, PathBuf};

    fn identity(path: &str) -> SourceIdentity {
        SourceIdentity::new(SourceKind::TargetMod, LogicalPath::parse(path).unwrap())
    }

    fn parsed(path: &str, source: &[u8]) -> ParsedFile {
        parse(identity(path), source)
    }

    fn definition(file: &ParsedFile, index: usize) -> &Field {
        file.definitions()
            .nth(index)
            .map(|(field, _)| field)
            .unwrap_or_else(|| panic!("definition {index} is absent from {:?}", file.source))
    }

    fn definition_names(file: &ParsedFile) -> Vec<String> {
        file.definitions()
            .map(|(field, _)| field.key.text().into_owned())
            .collect()
    }

    fn scalar(value: &Value) -> &Scalar {
        match value {
            Value::Scalar(scalar) => scalar,
            other => panic!("expected scalar, got {other:?}"),
        }
    }

    fn container(value: &Value) -> &Container {
        match value {
            Value::Container(container) => container,
            other => panic!("expected container, got {other:?}"),
        }
    }

    #[test]
    fn duplicates_and_every_operator_survive_in_source_order() {
        let file = parsed(
            "operators.txt",
            b"a = 1\na == 2\na != 3\na < 4\na <= 5\na > 6\na >= 7\na ?= 8\n",
        );

        assert!(file.faults.is_empty());
        assert_eq!(definition_names(&file), vec!["a"; 8]);
        let operators: Vec<_> = file
            .definitions()
            .map(|(field, _)| field.operator)
            .collect();
        assert_eq!(
            operators,
            [
                Operator::Equal,
                Operator::Exact,
                Operator::NotEqual,
                Operator::LessThan,
                Operator::LessThanEqual,
                Operator::GreaterThan,
                Operator::GreaterThanEqual,
                Operator::Exists,
            ]
        );
        assert_eq!(
            operators
                .iter()
                .map(|operator| operator.symbol())
                .collect::<Vec<_>>(),
            ["=", "==", "!=", "<", "<=", ">", ">=", "?="]
        );
    }

    #[test]
    fn exact_scalar_bytes_and_distinct_reference_kinds_survive() {
        let file = parsed(
            "scalars.txt",
            b"short = 0.1\nlong = 0.10\ndate = 2200.1.1\nreference = @cost\n\
              ordinary = @[ cost + 1 ]\nescaped = @\\[ cost + 1 ]\nparameter = $F$\n\
              name = \"Caf\xe9\"\n",
        );
        assert!(file.faults.is_empty(), "{:?}", file.faults);

        let observed: Vec<_> = file
            .definitions()
            .map(|(field, _)| {
                let scalar = scalar(&field.value);
                (scalar.kind, scalar.raw.clone())
            })
            .collect();
        assert_eq!(
            observed,
            [
                (ScalarKind::Number, b"0.1".to_vec()),
                (ScalarKind::Number, b"0.10".to_vec()),
                (ScalarKind::Unquoted, b"2200.1.1".to_vec()),
                (ScalarKind::VariableRef, b"@cost".to_vec()),
                (ScalarKind::VariableExpr, b"@[ cost + 1 ]".to_vec()),
                (ScalarKind::VariableExpr, b"@\\[ cost + 1 ]".to_vec()),
                (ScalarKind::Parameter, b"$F$".to_vec()),
                (ScalarKind::Quoted, b"Caf\xe9".to_vec()),
            ]
        );
    }

    #[test]
    fn containers_and_stellaris_dialect_keep_their_source_shape() {
        let file = parsed(
            "containers.txt",
            b"empty = {}\nobject = { a = 1 }\narray = { x y }\n\
              mixed = { flag a = 1 }\ntagged = rgb { 1 2 3 }\n\
              implied { rotation { 0 0 0 } unknown_token }\n\
              dialect = { flag [[X] value = $F$ ] [[!X] escaped = @\\[ cost + 1 ] ] \
              scope = [From.System.GetName] ordinary = @[ cost + 1 ] }\n",
        );
        assert!(file.faults.is_empty(), "{:?}", file.faults);

        assert_eq!(
            (0..4)
                .map(|index| container(&definition(&file, index).value).kind)
                .collect::<Vec<_>>(),
            [
                super::super::model::ContainerKind::Empty,
                super::super::model::ContainerKind::Object,
                super::super::model::ContainerKind::Array,
                super::super::model::ContainerKind::Mixed,
            ]
        );

        match &definition(&file, 4).value {
            Value::Tagged { tag, container, .. } => {
                assert_eq!(tag.raw, b"rgb");
                assert_eq!(container.kind, super::super::model::ContainerKind::Array);
            }
            other => panic!("expected tagged value, got {other:?}"),
        }

        let implied = definition(&file, 5);
        assert_eq!(implied.operator, Operator::Equal);
        assert_eq!(
            container(&implied.value).kind,
            super::super::model::ContainerKind::Mixed
        );

        let dialect = container(&definition(&file, 6).value);
        assert_eq!(dialect.kind, super::super::model::ContainerKind::Mixed);
        let conditionals: Vec<_> = dialect
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Conditional(conditional) => Some(conditional),
                Item::Field(_) | Item::Element(_) => None,
            })
            .collect();
        assert_eq!(conditionals.len(), 2);
        assert_eq!(conditionals[0].parameter, b"X");
        assert!(!conditionals[0].negated);
        assert!(conditionals[1].negated);
        let fields: Vec<_> = dialect.fields().collect();
        assert_eq!(scalar(&fields[0].value).kind, ScalarKind::ScopeToken);
        assert_eq!(scalar(&fields[1].value).kind, ScalarKind::VariableExpr);
    }

    #[test]
    fn every_valid_fixture_has_exact_ranges_and_only_clean_evidence() {
        for (source, bytes) in valid_fixtures() {
            let file = parse(source, &bytes);
            assert!(
                file.faults.is_empty(),
                "{} faulted: {:?}",
                file.source.logical,
                file.faults
            );
            assert!(
                file.items
                    .iter()
                    .all(|item| item.evidence == EvidenceQuality::Clean)
            );
            let faults = verify_ranges(&bytes, &file);
            assert!(
                faults.is_empty(),
                "{} has range faults: {faults:?}",
                file.source.logical
            );
        }
    }

    #[test]
    fn malformed_fixtures_report_faults_and_distinguish_clean_from_recovered_definitions() {
        let failures = malformed_fixture_failures(parse);
        assert!(failures.is_empty(), "{failures:#?}");
    }

    #[test]
    fn malformed_fixture_expectations_detect_disabled_resynchronization() {
        fn parse_without_resynchronization(source: SourceIdentity, data: &[u8]) -> ParsedFile {
            parse_with_boundary_finder(source, data, |_, _| None)
        }

        let failures = malformed_fixture_failures(parse_without_resynchronization);
        for name in [
            "fault_at_head.txt",
            "stray_close_brace.txt",
            "unclosed_brace_middle.txt",
            "unterminated_quote.txt",
        ] {
            assert!(
                failures.iter().any(|failure| failure.starts_with(name)),
                "{name} unexpectedly passed without resynchronization: {failures:#?}"
            );
        }
    }

    fn malformed_fixture_failures(parser: fn(SourceIdentity, &[u8]) -> ParsedFile) -> Vec<String> {
        let cases = [
            (
                "fault_at_head.txt",
                ParseFaultKind::ContainerUnclosed,
                Some("parser_second"),
                vec![
                    ("parser_second", EvidenceQuality::Recovered),
                    ("parser_third", EvidenceQuality::Recovered),
                    ("parser_fourth", EvidenceQuality::Recovered),
                ],
            ),
            (
                "stray_close_brace.txt",
                ParseFaultKind::UnexpectedCloseBrace,
                Some("parser_third"),
                vec![
                    ("parser_first", EvidenceQuality::Clean),
                    ("parser_second", EvidenceQuality::Clean),
                    ("parser_third", EvidenceQuality::Recovered),
                    ("parser_fourth", EvidenceQuality::Recovered),
                ],
            ),
            (
                "truncated_tail.txt",
                ParseFaultKind::ContainerUnclosed,
                None,
                vec![
                    ("parser_first", EvidenceQuality::Clean),
                    ("parser_second", EvidenceQuality::Clean),
                    ("parser_third", EvidenceQuality::Clean),
                ],
            ),
            (
                "unclosed_brace_middle.txt",
                ParseFaultKind::ContainerUnclosed,
                Some("parser_third"),
                vec![
                    ("parser_first", EvidenceQuality::Clean),
                    ("parser_third", EvidenceQuality::Recovered),
                    ("parser_fourth", EvidenceQuality::Recovered),
                ],
            ),
            (
                "unterminated_quote.txt",
                ParseFaultKind::ContainerUnclosed,
                Some("parser_third"),
                vec![
                    ("parser_first", EvidenceQuality::Clean),
                    ("parser_third", EvidenceQuality::Recovered),
                    ("parser_fourth", EvidenceQuality::Recovered),
                ],
            ),
        ];
        let mut failures = Vec::new();

        for (name, expected_kind, resumes_at, expected_definitions) in cases {
            let bytes = fs::read(malformed_root().join(name)).unwrap();
            let file = parser(identity(name), &bytes);
            if file.faults.len() != 1 {
                failures.push(format!("{name}: expected one fault, got {:?}", file.faults));
                continue;
            }
            let fault = &file.faults[0];
            if fault.kind != expected_kind {
                failures.push(format!(
                    "{name}: expected {expected_kind:?}, got {:?}",
                    fault.kind
                ));
            }
            if fault.position > bytes.len() as u64 {
                failures.push(format!(
                    "{name}: fault position {} exceeds {} bytes",
                    fault.position,
                    bytes.len()
                ));
            }
            let expected_boundary =
                resumes_at.map(|needle| find_line_start(&bytes, needle.as_bytes()) as u64);
            if fault.recovery_boundary != expected_boundary {
                failures.push(format!(
                    "{name}: expected recovery boundary {expected_boundary:?}, got {:?}",
                    fault.recovery_boundary
                ));
            }
            let range_faults = verify_ranges(&bytes, &file);
            if !range_faults.is_empty() {
                failures.push(format!("{name}: recovered ranges failed: {range_faults:?}"));
            }
            let observed_definitions = file
                .definitions()
                .map(|(field, evidence)| (field.key.text().into_owned(), evidence))
                .collect::<Vec<_>>();
            let expected_definitions = expected_definitions
                .into_iter()
                .map(|(name, evidence)| (name.to_owned(), evidence))
                .collect::<Vec<_>>();
            if observed_definitions != expected_definitions {
                failures.push(format!(
                    "{name}: expected definitions {expected_definitions:?}, got \
                     {observed_definitions:?}"
                ));
            }
        }

        failures
    }

    fn find_line_start(haystack: &[u8], needle: &[u8]) -> usize {
        haystack
            .windows(needle.len())
            .enumerate()
            .find_map(|(index, window)| {
                (window == needle && (index == 0 || haystack[index - 1] == b'\n')).then_some(index)
            })
            .unwrap_or_else(|| panic!("{needle:?} is absent"))
    }

    #[test]
    fn repeated_faults_are_reported_in_order_and_recovery_quality_never_returns_to_clean() {
        let source = b"first = { cost = 100 } }\n\
                       second = { cost = 200 } }\n\
                       third = { cost = 300 }\n";
        let file = parsed("two-faults.txt", source);

        assert_eq!(
            file.faults
                .iter()
                .map(|fault| fault.kind)
                .collect::<Vec<_>>(),
            [
                ParseFaultKind::UnexpectedCloseBrace,
                ParseFaultKind::UnexpectedCloseBrace,
            ]
        );
        assert!(file.faults[0].position < file.faults[1].position);
        assert_eq!(
            file.faults
                .iter()
                .map(|fault| fault.recovery_boundary)
                .collect::<Vec<_>>(),
            [
                Some(find_line_start(source, b"second") as u64),
                Some(find_line_start(source, b"third") as u64),
            ]
        );
        assert_eq!(
            file.definitions()
                .map(|(field, evidence)| (field.key.text().into_owned(), evidence))
                .collect::<Vec<_>>(),
            [
                ("first".to_owned(), EvidenceQuality::Clean),
                ("second".to_owned(), EvidenceQuality::Recovered),
                ("third".to_owned(), EvidenceQuality::Recovered),
            ]
        );
    }

    #[derive(Debug, Clone, Copy)]
    enum InjectedFault {
        RemoveCloseBrace,
        AddCloseBrace,
        OpenQuote,
    }

    fn generated_fixture(definition_count: usize) -> Vec<u8> {
        (0..definition_count)
            .map(|index| format!("parser_{index} = {{ cost = {index} }}\n"))
            .collect::<String>()
            .into_bytes()
    }

    fn inject_fault(
        source: &[u8],
        clean: &ParsedFile,
        definition_index: usize,
        fault: InjectedFault,
    ) -> Vec<u8> {
        let range = clean
            .definitions()
            .nth(definition_index)
            .map(|(field, _)| field.range.as_usize().unwrap())
            .unwrap();
        let mut mutated = source.to_vec();
        match fault {
            InjectedFault::RemoveCloseBrace => {
                assert_eq!(mutated[range.end - 1], b'}');
                mutated.remove(range.end - 1);
            }
            InjectedFault::AddCloseBrace => mutated.insert(range.end, b'}'),
            InjectedFault::OpenQuote => {
                let brace = source[range.clone()]
                    .iter()
                    .position(|byte| *byte == b'{')
                    .unwrap();
                mutated.insert(range.start + brace + 1, b'"');
            }
        }
        mutated
    }

    proptest! {
        #[test]
        fn one_injected_fault_loses_at_most_its_definition(
            definition_count in 1usize..12,
            arbitrary_index in any::<usize>(),
            fault in prop_oneof![
                Just(InjectedFault::RemoveCloseBrace),
                Just(InjectedFault::AddCloseBrace),
                Just(InjectedFault::OpenQuote),
            ],
        ) {
            let faulted_index = arbitrary_index % definition_count;
            let source = generated_fixture(definition_count);
            let clean = parsed("generated-clean.txt", &source);
            let mutated = inject_fault(&source, &clean, faulted_index, fault);
            let file = parsed("generated-fault.txt", &mutated);
            let observed = definition_names(&file);

            prop_assert!(!file.faults.is_empty());
            prop_assert!(observed.len() + 1 >= definition_count);
            for index in 0..definition_count {
                let name = format!("parser_{index}");
                if index != faulted_index || matches!(fault, InjectedFault::AddCloseBrace) {
                    prop_assert!(observed.contains(&name), "{name} was outside the faulted definition");
                }
            }
            prop_assert!(
                observed
                    .iter()
                    .all(|name| name.strip_prefix("parser_").is_some()),
                "recovery invented a definition: {observed:?}"
            );
            if matches!(fault, InjectedFault::AddCloseBrace) {
                prop_assert!(
                    file.faults
                        .iter()
                        .any(|fault| fault.kind == ParseFaultKind::UnexpectedCloseBrace)
                );
            }
        }
    }

    #[test]
    fn truncating_the_last_definition_loses_exactly_one_and_cannot_resume() {
        let source = generated_fixture(4);
        let clean = parsed("truncate-clean.txt", &source);
        let last = clean.definitions().count() - 1;
        let range = clean
            .definitions()
            .nth(last)
            .map(|(field, _)| field.range.as_usize().unwrap())
            .unwrap();
        let mut truncated = source;
        truncated.truncate(range.end - 1);

        let file = parsed("truncate-last.txt", &truncated);
        assert_eq!(file.definitions().count(), clean.definitions().count() - 1);
        assert_eq!(
            definition_names(&file),
            ["parser_0", "parser_1", "parser_2"]
        );
        assert_eq!(file.faults.len(), 1);
        assert_eq!(file.faults[0].recovery_boundary, None);
    }

    #[test]
    fn brace_faults_are_typed_and_attributed_to_the_faulting_brace() {
        let bytes = fs::read(malformed_root().join("unclosed_brace_middle.txt")).unwrap();
        let file = parse(identity("unclosed_brace_middle.txt"), &bytes);
        let fault = &file.faults[0];
        assert_eq!(fault.kind, ParseFaultKind::ContainerUnclosed);
        assert_eq!(bytes.get(fault.position as usize), Some(&b'{'));

        let bytes = fs::read(malformed_root().join("stray_close_brace.txt")).unwrap();
        let file = parse(identity("stray_close_brace.txt"), &bytes);
        let fault = &file.faults[0];
        assert_eq!(fault.kind, ParseFaultKind::UnexpectedCloseBrace);
        assert_eq!(bytes.get(fault.position as usize), Some(&b'}'));
    }

    #[test]
    fn a_valid_looking_stray_token_is_retained_without_an_invented_fault() {
        let bytes = fs::read(malformed_root().join("stray_token_at_head.txt")).unwrap();
        let file = parse(identity("stray_token_at_head.txt"), &bytes);
        assert!(file.faults.is_empty());
        assert!(matches!(
            &file.items[0].item,
            Item::Element(Value::Scalar(Scalar {
                raw,
                kind: ScalarKind::Number,
                ..
            })) if raw == b"3407"
        ));
        assert_eq!(
            definition_names(&file),
            ["parser_first", "parser_second", "parser_third"]
        );
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

    fn malformed_root() -> PathBuf {
        fixture_root().join("malformed")
    }

    fn fixture_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("fixtures")
            .join("parser")
    }
}
