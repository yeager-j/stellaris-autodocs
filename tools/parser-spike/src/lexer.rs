//! Path B: adapt Jomini's `TokenReader` into the application-owned model, with spans.
//!
//! The spike's [`Known Jomini constraints`](../../../docs/spikes/parser-evaluation.md)
//! section states that source positions are unavailable through supported APIs. That holds
//! for the tape. It does not hold for `TokenReader`, which is publicly exported and whose
//! `position()` returns the byte offset of the data stream consumed so far — for every
//! token, braces and operators included.
//!
//! How that offset becomes a span took measurement rather than reading. `examples/probe.rs`
//! shows two behaviours that rule out the obvious derivation: the reader has not yet
//! consumed the whitespace and comments preceding a token when `position()` is read *before*
//! it, and after reading an unquoted token it has sometimes consumed one trailing boundary
//! byte as well. So `end - width` is wrong, sporadically, in a way that a small fixture set
//! would not reveal.
//!
//! What holds in every observed case is that the position taken *before* a read is a lower
//! bound on the token's start. Skipping the trivia that Clausewitz itself skips — a byte
//! order mark, whitespace, and `#` comments to end of line — lands exactly on the token, and
//! its width is then known from the token itself.
//!
//! The derivation is not taken on trust. Every span this module produces is re-sliced from
//! the original bytes and compared against the token it claims to cover, by
//! [`verify_spans`] in tests and by the `p2-ranges` run across the whole corpus. A
//! derivation that is right about 99% of a corpus is a bug, not a technique, and only an
//! exhaustive round-trip separates the two.
//!
//! Being incremental, the reader also survives a syntax fault: it reports the byte offset
//! where it gave up, so parsing can resume at the next top-level definition instead of
//! discarding the file. `TextTape::from_slice` cannot, and the difference between them is
//! the blast-radius measurement.

use crate::classify;
use crate::model::{
    Conditional, Container, Fault, Field, Item, Operator, ParsedFile, Scalar, ScalarKind, Span,
    Value,
};
use jomini::text::{Operator as JominiOperator, Token, TokenReader};

/// Guards against a pathological or hostile nesting depth.
///
/// Recursive descent over untrusted mod source would otherwise let a file of open braces
/// exhaust the stack, which is an availability failure rather than a parse failure. The
/// census in `p1-coverage` reports the deepest nesting actually observed, so this number
/// can be justified against the corpus instead of guessed.
const MAX_DEPTH: usize = 256;

/// A syntax fault, with the byte offset the reader reached.
///
/// Jomini's own `ReaderError` carries a position, but positions raised by this module —
/// an unexpected close brace, exhausted depth — need one too, and a fault whose offset is
/// zero would silently break both the diagnostic and the resynchronization that depends on
/// it. So both kinds are carried in one type that always has a real offset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexError {
    pub message: String,
    pub offset: usize,
    /// The input ran out. Distinguished because an unclosed brace is only *detected* at end
    /// of file while it *happened* at the brace, and resuming from the detection point
    /// would abandon the rest of the file for a fault near its start.
    pub eof: bool,
}

pub fn parse(data: &[u8]) -> ParsedFile {
    let mut items = Vec::new();
    let mut faults = Vec::new();
    let mut base = 0usize;

    loop {
        let mut cursor = Cursor::new(&data[base..], base);
        match parse_items(&mut cursor, 0, Until::EndOfFile) {
            Ok(mut parsed) => {
                items.append(&mut parsed);
                break;
            }
            Err(mut stop) => {
                items.append(&mut stop.items);
                // A resume point must move forward. An unclosed brace attributes its fault
                // backwards to the brace itself, so without this the loop could re-enter
                // the same region.
                let resume = resync(data, stop.error.offset).filter(|at| *at > base);
                faults.push(Fault {
                    message: stop.error.message,
                    offset: stop.error.offset as u32,
                    resumed_at: resume.map(|at| at as u32),
                    abandoned: stop.abandoned as u32,
                });
                match resume {
                    Some(at) => base = at,
                    None => break,
                }
            }
        }
    }

    ParsedFile { items, faults }
}

/// Where to resume after a fault.
///
/// Stellaris script writes every top-level definition flush against column zero, so the
/// next line beginning with an identifier character is the next definition. A line
/// beginning with `}` is skipped: it is the tail of whatever construct just failed, and
/// resuming there would fault again immediately on an unmatched close.
///
/// This is a heuristic about layout convention, not about grammar. It can only lose
/// definitions, never invent them — everything it yields still had to parse — and the
/// blast-radius run reports what it actually recovers rather than what it ought to.
fn resync(data: &[u8], after: usize) -> Option<usize> {
    let mut index = after.min(data.len());

    // Move past the remainder of the offending line first, so a fault sitting on a
    // column-zero definition cannot resume onto itself and loop.
    while index < data.len() && data[index] != b'\n' {
        index += 1;
    }

    while index < data.len() {
        index += 1;
        match data.get(index) {
            None => return None,
            Some(&first)
                if first.is_ascii_alphabetic() || first == b'_' || first == b'@' || first == b'"' =>
            {
                return Some(index)
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

/// A fault, plus everything that had already parsed cleanly before it.
///
/// Carrying the partial items out is the point: discarding them would reproduce the tape's
/// all-or-nothing failure inside the adapter that exists to avoid it.
struct Stop {
    error: LexError,
    items: Vec<Item>,
    /// Items parsed inside the incomplete construct and then discarded.
    ///
    /// They are counted rather than kept: hoisting a broken container's children into its
    /// parent would report structure the source does not contain, which is the failure mode
    /// this adapter exists to avoid, not one to imitate more cheaply.
    abandoned: usize,
}

impl Stop {
    fn bare(error: LexError) -> Self {
        Stop {
            error,
            items: Vec::new(),
            abandoned: 0,
        }
    }
}

/// What ends the item sequence being read.
///
/// Conditional blocks close with `]` rather than `}`, so the terminator has to be stated
/// rather than inferred from depth: a `]` inside a container is the end of a conditional,
/// and a `}` inside a conditional is the end of the container that encloses it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Until {
    EndOfFile,
    CloseBrace,
    CloseBracket,
}

fn parse_items(cursor: &mut Cursor<'_>, depth: usize, until: Until) -> Result<Vec<Item>, Stop> {
    let mut items: Vec<Item> = Vec::new();

    let mut abandoned = 0usize;

    macro_rules! fail {
        ($error:expr) => {
            return Err(Stop {
                error: $error,
                items,
                abandoned,
            })
        };
    }
    macro_rules! nested {
        ($result:expr) => {
            match $result {
                Ok(value) => value,
                Err(stop) => {
                    abandoned += stop.abandoned;
                    fail!(stop.error)
                }
            }
        };
    }

    loop {
        // Re-assert the scope before every read: `]` closes a block only when the innermost
        // scope is a conditional, and a nested container or conditional may have changed it.
        cursor.set_conditional_depth(usize::from(until == Until::CloseBracket));

        let lexeme = match cursor.advance() {
            Ok(Some(lexeme)) => lexeme,
            Ok(None) if until == Until::EndOfFile => return Ok(items),
            Ok(None) => fail!(cursor.eof("unexpected end of file inside a container")),
            Err(error) => fail!(error),
        };

        match lexeme.lex {
            Lex::Close if until == Until::CloseBrace => return Ok(items),
            Lex::Close => fail!(at(lexeme.span, "close brace with nothing open")),
            Lex::Operator(_) => fail!(at(lexeme.span, "operator without a key")),
            Lex::ConditionalClose if until == Until::CloseBracket => return Ok(items),
            Lex::ConditionalClose => fail!(at(lexeme.span, "`]` with no conditional open")),
            Lex::ConditionalOpen { parameter, negated } => {
                let body = nested!(parse_items(cursor, depth + 1, Until::CloseBracket));
                items.push(Item::Conditional(Conditional {
                    parameter,
                    negated,
                    items: body,
                    span: Some(Span::new(lexeme.span.start as usize, cursor.position())),
                }));
            }
            Lex::Open => {
                let container = nested!(container(cursor, lexeme.span, depth));
                items.push(Item::Element(Value::Container(container)));
            }
            Lex::Scalar(scalar) => {
                // `key { … }` with no operator is an assignment. Vanilla writes it — the
                // last definition of `common/named_colors/01_trait_colors.txt` is
                // `trait_bg_active_glow { … }`, and `.asset` entities write
                // `rotation {0 0 0}` — and the tape reads it as a pair everywhere in item
                // position, reserving the tagged-literal reading for value position.
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
                    items.push(Item::Element(Value::Scalar(scalar.into_model(lexeme.span))));
                    continue;
                };

                if explicit {
                    cursor.drop_peeked();
                }
                let key = scalar.into_model(lexeme.span);
                let value = nested!(value(cursor, depth));
                let span = value
                    .span()
                    .map(|end| Span::new(lexeme.span.start as usize, end.end as usize));
                items.push(Item::Field(Field {
                    key,
                    operator,
                    value,
                    span,
                }));
            }
        }
    }
}

/// Read one value, which is what follows an operator.
///
/// Tagged literals such as `color = rgb { 100 200 50 }` are recognized only here, in value
/// position. In element position the same two tokens are two items: `{ a { b } }` is an
/// array of a scalar and a container, and reading it as one tagged value would invent
/// structure the source never stated.
fn value(cursor: &mut Cursor<'_>, depth: usize) -> Result<Value, Stop> {
    let lexeme = match cursor.advance() {
        Ok(Some(lexeme)) => lexeme,
        Ok(None) => return Err(Stop::bare(cursor.eof("value expected, found end of file"))),
        Err(error) => return Err(Stop::bare(error)),
    };

    match lexeme.lex {
        Lex::Open => Ok(Value::Container(container(cursor, lexeme.span, depth)?)),
        Lex::Scalar(scalar) => {
            let tagged = match cursor.peek() {
                Ok(Some(Lexeme { lex: Lex::Open, .. })) => true,
                Ok(_) => false,
                Err(error) => return Err(Stop::bare(error)),
            };
            if !tagged {
                return Ok(Value::Scalar(scalar.into_model(lexeme.span)));
            }

            let open = cursor.take_peeked();
            let container = container(cursor, open.span, depth)?;
            let span = container
                .span
                .map(|end| Span::new(lexeme.span.start as usize, end.end as usize));
            Ok(Value::Tagged {
                tag: scalar.into_model(lexeme.span),
                container,
                span,
            })
        }
        Lex::Close => Err(Stop::bare(at(lexeme.span, "value expected, found `}`"))),
        Lex::Operator(_) => Err(Stop::bare(at(lexeme.span, "value expected, found operator"))),
        Lex::ConditionalOpen { .. } | Lex::ConditionalClose => Err(Stop::bare(at(
            lexeme.span,
            "value expected, found a conditional-block bracket",
        ))),
    }
}

fn container(cursor: &mut Cursor<'_>, open: Span, depth: usize) -> Result<Container, Stop> {
    if depth >= MAX_DEPTH {
        return Err(Stop::bare(at(
            open,
            &format!("nesting deeper than {MAX_DEPTH} levels"),
        )));
    }

    let items = match parse_items(cursor, depth + 1, Until::CloseBrace) {
        Ok(items) => items,
        Err(mut stop) => {
            stop.abandoned += stop.items.len();
            stop.items = Vec::new();
            // An unclosed brace is detected at end of file but happened here. Attributing it
            // to the detection point would throw away every definition after the brace for a
            // fault that may sit near the top of the file; each enclosing level rewrites in
            // turn, so the outermost unclosed brace is what recovery finally resumes from.
            if stop.error.eof {
                stop.error.offset = open.start as usize;
                stop.error.message = "container never closed".to_owned();
                stop.error.eof = false;
            }
            return Err(stop);
        }
    };

    // The close brace was the last token consumed, so the container ends where the cursor
    // now stands.
    let span = Span::new(open.start as usize, cursor.position());
    Ok(Container::from_items(items, Some(span)))
}

fn at(span: Span, message: &str) -> LexError {
    LexError {
        message: message.to_owned(),
        offset: span.start as usize,
        eof: false,
    }
}

impl Value {
    fn span(&self) -> Option<Span> {
        match self {
            Value::Scalar(scalar) => scalar.span,
            Value::Container(container) => container.span,
            Value::Tagged { span, .. } => *span,
        }
    }
}

#[derive(Clone)]
struct RawScalar {
    kind: ScalarKind,
    raw: Vec<u8>,
}

impl RawScalar {
    fn into_model(self, span: Span) -> Scalar {
        Scalar {
            kind: self.kind,
            raw: self.raw,
            span: Some(span),
        }
    }
}

#[derive(Clone)]
enum Lex {
    Open,
    Close,
    Operator(Operator),
    Scalar(RawScalar),
    /// `[[NAME]` or `[[!NAME]`, opening a conditional-compilation block.
    ConditionalOpen { parameter: Vec<u8>, negated: bool },
    /// `]`, closing one.
    ConditionalClose,
}

/// Constructs `TokenReader` does not recognize, lexed from the bytes before delegating.
///
/// The reader is written for Paradox save games, where neither construct occurs. Given
/// `[[EXTRA] add = $EXTRA$ ]` it yields `[`, `[EXTRA`, `]`, `add`, `=`, `$EXTRA$`, `]` as
/// seven unquoted scalars, and given `[[!A]` it turns the negation mark into a `!=`
/// operator. Given `@\[ 1 + 2 ]` it splits the expression into six tokens. All three are
/// real Stellaris syntax — vanilla `common/script_values` and `common/scripted_effects` use
/// the first two, and the escaped expression is what makes `TextTape` reject eight vanilla
/// files outright.
///
/// So the wrapper cannot simply consume Jomini's token stream: it has to recognize these
/// itself and tell the reader to skip the bytes. That cost is a finding, not an
/// implementation detail, and it is deliberately confined to this one function so the
/// finding can state its size.
fn stellaris_construct(data: &[u8], at: usize, inside_conditional: bool) -> Option<(Lex, usize)> {
    match data.get(at)? {
        // A single `[` opens a runtime scope reference. `TokenReader` splits it at the
        // closing bracket and `TextTape` splits it into three tokens; neither reading
        // survives into a form the resolver could recognize as one reference.
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
        // Only when a conditional is open. Otherwise `]` is ordinary content: vanilla
        // `scripted_loc` and Gigastructural events write `variable_string = [From.GetName]`,
        // and closing an imaginary block there desynchronized the rest of the file.
        b']' if inside_conditional => Some((Lex::ConditionalClose, 1)),
        // `@\[ … ]` is one expression over scripted constants, not a sequence of tokens.
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
    span: Span,
}

/// One-token lookahead over `TokenReader`, producing file-relative spans.
///
/// Lookahead is unavoidable: `key = value` and a bare element differ only by whether an
/// operator follows the first scalar. Each token is copied into owned bytes as it is read,
/// both because `Token<'_>` borrows the reader and because the model owns its bytes.
struct Cursor<'a> {
    reader: TokenReader<'a>,
    /// The same bytes the reader is consuming, so a token's start can be located from the
    /// position recorded before it was read.
    data: &'a [u8],
    peeked: Option<Lexeme>,
    /// Offset of this cursor's slice within the whole file, so spans stay file-relative
    /// across a resynchronized restart.
    base: usize,
    last: usize,
    /// How many `[[NAME]` blocks are currently open, which decides whether `]` closes one.
    conditionals: usize,
}

impl<'a> Cursor<'a> {
    fn new(data: &'a [u8], base: usize) -> Self {
        Cursor {
            reader: TokenReader::from_slice(data),
            data,
            peeked: None,
            base,
            last: base,
            conditionals: 0,
        }
    }

    /// Declare whether the scope being read is a conditional block, which is what decides
    /// how `]` is classified.
    ///
    /// Lookahead is invalidated only when it is a `]` in either of its readings. Discarding
    /// any other peeked token would lose content, since nothing else is depth-dependent.
    fn set_conditional_depth(&mut self, depth: usize) {
        if self.conditionals == depth {
            return;
        }
        self.conditionals = depth;
        let bracket = matches!(
            &self.peeked,
            Some(Lexeme {
                lex: Lex::ConditionalClose,
                ..
            })
        ) || matches!(
            &self.peeked,
            Some(Lexeme { lex: Lex::Scalar(scalar), .. }) if scalar.raw == b"]"
        );
        if bracket {
            self.peeked = None;
        }
    }

    /// End of the most recently consumed token.
    fn position(&self) -> usize {
        self.last
    }

    fn eof(&self, message: &str) -> LexError {
        LexError {
            message: message.to_owned(),
            offset: self.base + self.reader.position(),
            eof: true,
        }
    }

    fn peek(&mut self) -> Result<Option<&Lexeme>, LexError> {
        if self.peeked.is_none() {
            self.peeked = self.read()?;
        }
        Ok(self.peeked.as_ref())
    }

    /// Consume a lexeme the caller has already inspected through [`peek`].
    fn take_peeked(&mut self) -> Lexeme {
        let lexeme = self
            .peeked
            .take()
            .expect("take_peeked follows a successful peek");
        self.last = lexeme.span.end as usize;
        lexeme
    }

    fn drop_peeked(&mut self) {
        let _ = self.take_peeked();
    }

    fn advance(&mut self) -> Result<Option<Lexeme>, LexError> {
        if self.peeked.is_some() {
            return Ok(Some(self.take_peeked()));
        }
        let lexeme = self.read()?;
        if let Some(lexeme) = &lexeme {
            self.last = lexeme.span.end as usize;
        }
        Ok(lexeme)
    }

    /// Read one token and give it a span.
    ///
    /// The position taken before the read bounds where the token can begin; skipping the
    /// trivia Clausewitz skips lands on it exactly. Width comes from the token — one byte
    /// for a brace, the symbol width for an operator, the scalar's length when unquoted,
    /// and two more than that when quoted, since the borrowed bytes exclude the delimiters
    /// but the span names the text the file contains.
    fn read(&mut self) -> Result<Option<Lexeme>, LexError> {
        let opens_at = skip_trivia(self.data, self.reader.position());

        if let Some((lex, width)) = stellaris_construct(self.data, opens_at, self.conditionals > 0) {
            // Consume the trivia and the construct together, so the reader resumes exactly
            // after it. `read_bytes` is the supported way to advance the source by a known
            // amount without asking it to tokenize what it would tokenize wrongly.
            let consume = opens_at - self.reader.position() + width;
            self.reader.read_bytes(consume).map_err(|error| LexError {
                message: format!("{:?}", error.kind()),
                offset: self.base + error.position(),
                eof: true,
            })?;
            let start = self.base + opens_at;
            return Ok(Some(Lexeme {
                lex,
                span: Span::new(start, start + width),
            }));
        }

        let converted = {
            let token = match self.reader.next() {
                Ok(Some(token)) => token,
                Ok(None) => return Ok(None),
                Err(error) => {
                    let kind = error.kind();
                    return Err(LexError {
                        message: format!("{kind:?}"),
                        offset: self.base + error.position(),
                        eof: matches!(kind, jomini::text::ReaderErrorKind::Eof),
                    });
                }
            };
            match token {
                Token::Open => (Lex::Open, 1usize),
                Token::Close => (Lex::Close, 1usize),
                Token::Operator(operator) => {
                    let width = operator.symbol().len();
                    (Lex::Operator(convert_operator(operator)), width)
                }
                Token::Unquoted(scalar) => {
                    let raw = scalar.as_bytes().to_vec();
                    let width = raw.len();
                    (
                        Lex::Scalar(RawScalar {
                            kind: classify::unquoted(&raw),
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

        let (lex, width) = converted;
        let start = self.base + opens_at;
        Ok(Some(Lexeme {
            lex,
            span: Span::new(start, start + width),
        }))
    }
}

/// Advance past a byte order mark, whitespace, `;`, and `#` comments, in the order a
/// Clausewitz reader would.
///
/// `;` is a separator rather than content — Jomini's own character-class table marks it a
/// token boundary alongside whitespace — and vanilla writes it in `.gui` files and in the
/// prose that shares the script directories. Leaving it out shifted every following span by
/// its width, which the corpus round-trip caught and no small fixture would have.
///
/// A comment runs to end of line, so it is skipped wholesale rather than scanned for
/// content: a `#` inside a quoted string is never reached here, because this only ever runs
/// at a position where a token has not yet started.
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

/// A span that does not re-slice to the text it claims to cover is a defect, so every span
/// is checked against the source rather than assumed correct.
///
/// Returns the offending spans; empty when every one round-trips.
pub fn verify_spans(data: &[u8], file: &ParsedFile) -> Vec<SpanFault> {
    let mut faults = Vec::new();
    for item in &file.items {
        check_item(data, item, &mut faults);
    }
    faults
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpanFault {
    pub span: Span,
    pub expected: String,
    pub found: String,
}

fn check_item(data: &[u8], item: &Item, faults: &mut Vec<SpanFault>) {
    match item {
        Item::Field(field) => {
            check_scalar(data, &field.key, faults);
            check_value(data, &field.value, faults);
        }
        Item::Element(value) => check_value(data, value, faults),
        Item::Conditional(conditional) => {
            for item in &conditional.items {
                check_item(data, item, faults);
            }
        }
    }
}

fn check_value(data: &[u8], value: &Value, faults: &mut Vec<SpanFault>) {
    match value {
        Value::Scalar(scalar) => check_scalar(data, scalar, faults),
        Value::Container(container) => check_container(data, container, faults),
        Value::Tagged { tag, container, .. } => {
            check_scalar(data, tag, faults);
            check_container(data, container, faults);
        }
    }
}

fn check_container(data: &[u8], container: &Container, faults: &mut Vec<SpanFault>) {
    if let Some(span) = container.span {
        let slice = data.get(span.range()).unwrap_or_default();
        if slice.first() != Some(&b'{') || slice.last() != Some(&b'}') {
            faults.push(SpanFault {
                span,
                expected: "{ … }".into(),
                found: preview(slice),
            });
        }
    }
    for item in &container.items {
        check_item(data, item, faults);
    }
}

fn check_scalar(data: &[u8], scalar: &Scalar, faults: &mut Vec<SpanFault>) {
    let Some(span) = scalar.span else {
        return;
    };
    let slice = data.get(span.range()).unwrap_or_default();
    let expected: &[u8] = &scalar.raw;
    let round_trips = match scalar.kind {
        ScalarKind::Quoted => {
            slice.len() == expected.len() + 2
                && slice.first() == Some(&b'"')
                && slice.last() == Some(&b'"')
                && &slice[1..slice.len() - 1] == expected
        }
        _ => slice == expected,
    };
    if !round_trips {
        faults.push(SpanFault {
            span,
            expected: String::from_utf8_lossy(expected).into_owned(),
            found: preview(slice),
        });
    }
}

fn preview(slice: &[u8]) -> String {
    String::from_utf8_lossy(&slice[..slice.len().min(48)]).into_owned()
}
