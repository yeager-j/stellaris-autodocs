//! The serializable Result contract, and the one place an unexpected failure may reach a
//! wire (docs/technical-design.md, "Serializable result contract"; docs/decision-log.md,
//! P-004).
//!
//! Expected application outcomes cross both transports as plain data:
//!
//! ```text
//! { "ok": true,  "value": ... }
//! { "ok": false, "error": ... }
//! ```
//!
//! Rust keeps native typed [`Result`]s internally; this module is where one becomes that
//! envelope. It sits in `transport` rather than beside [`error`](crate::error) deliberately:
//! deep modules must not be able to serialize a failure at all, and Phase 11's Companion HTTP
//! adapter emits the identical envelope, which is the second consumer that earns the shared
//! home.
//!
//! # Redaction is structural here, not conventional
//!
//! [`Unexpected`] derives `Debug` **including** its private message, while its `Display` is
//! redacted — so any seam reaching for `{:?}` leaks source content, absolute paths, or
//! whatever a caller put in the message. A Phase 0 review recorded that a comment saying "do
//! not do that" is not a control.
//!
//! The control is [`Rejection`]: it has two fields, one a fieldless enum and one a
//! [`CorrelationId`] (sixteen bytes), and no way to construct one except from an
//! [`Unexpected`] that it immediately drops. Message detail is *unrepresentable* in it rather
//! than merely unwritten, and [`resolve`] is the only function that turns a failure into a
//! response, so there is exactly one path from the unexpected channel to a transport. What
//! remains conventional — that nobody adds `impl Serialize for Unexpected` — is asserted by
//! `error.rs::no_source_file_makes_unexpected_serializable`, which is a text gate and says so.

use crate::error::{CorrelationId, Failure, OpResult, Unexpected};
use serde::Serialize;
use serde::ser::{SerializeMap, Serializer};

/// One expected application outcome, in the shape both transports emit.
///
/// **Hand-written `Serialize`, and it must stay that way.** serde's internally tagged
/// representation writes the *variant name* as the tag value, so `#[serde(tag = "ok")]` can
/// only ever emit `"ok":"Value"` — the contract fixes `ok` as a boolean. `serialize_map` rather
/// than `serialize_struct` for the related reason that a struct promises a statically known
/// field list and this one's second key depends on the variant.
///
/// **No `Deserialize`, deliberately.** A Rust decoder here would be a second statement of the
/// wire shape inside the crate that produces it, and a round-trip test can be green with both
/// halves wrong. The real second implementation is the TypeScript documentation client's
/// decoder; the cross-language suite is what compares them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Envelope<T, E> {
    Value(T),
    Refusal(E),
}

impl<T: Serialize, E: Serialize> Serialize for Envelope<T, E> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(2))?;
        match self {
            // Nothing here is ever conditional on the payload. `skip_serializing_if` is exactly
            // how `value` would disappear for a void success, which the design names as the
            // hazard: "a void success uses `null` or an explicit object rather than JavaScript
            // `undefined`, whose property would disappear during JSON encoding".
            Self::Value(value) => {
                map.serialize_entry("ok", &true)?;
                map.serialize_entry("value", value)?;
            }
            Self::Refusal(error) => {
                map.serialize_entry("ok", &false)?;
                map.serialize_entry("error", error)?;
            }
        }
        map.end()
    }
}

/// Why a command rejected instead of resolving.
///
/// Neither field can hold text. That is the whole design: a message cannot be omitted from a
/// rejection by accident because there is nowhere in one to put it.
///
/// The fixed `kind` is not decoration. The design requires cross-language negative controls
/// "for missing discriminants", and a Tauri command's *own* framework rejections — unknown
/// command, an argument that fails to deserialize — arrive as bare JSON strings; without a
/// discriminant the frontend would have to guess ours apart from the framework's by shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rejection {
    kind: RejectionKind,
    correlation: CorrelationId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RejectionKind {
    Unexpected,
}

impl Rejection {
    /// The identifier a person reads back to support, and the only thing this type carries
    /// about the failure it came from.
    pub fn correlation(&self) -> CorrelationId {
        self.correlation
    }
}

impl From<Unexpected> for Rejection {
    /// Takes the identifier, records the detail, and drops the failure.
    fn from(unexpected: Unexpected) -> Self {
        record(&unexpected);
        Self {
            kind: RejectionKind::Unexpected,
            correlation: unexpected.correlation(),
        }
    }
}

/// The whole of "detailed error chains remain only in protected desktop logs" today: one line
/// on stderr, at the same instant and in the same function as the identifier that indexes it.
///
/// Doing nothing instead would be worse than a missing log — a correlation identifier that
/// correlates with nothing is a lie told to whoever reads it off the screen. What is still
/// missing is stated rather than implied: there is no file destination, no rotation, and no
/// log-redaction policy for credentials or paths, and on a packaged Windows release
/// `main.rs`'s `windows_subsystem = "windows"` means there is no console at all, so the detail
/// is lost and only the identifier survives. When a real sink lands, this function is the one
/// call site to change.
fn record(unexpected: &Unexpected) {
    eprintln!("{}", unexpected.log_detail());
}

impl Serialize for Rejection {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry(
            "kind",
            match self.kind {
                RejectionKind::Unexpected => "unexpected",
            },
        )?;
        // Rendered through `Display`, which is `error`'s redacted form and already the crate's
        // one lowercase-hex spelling. Keeping serde out of `error` is what makes "a deep module
        // cannot serialize a failure" visible rather than merely true.
        map.serialize_entry("correlationId", &self.correlation.to_string())?;
        map.end()
    }
}

/// The only mapping from an application outcome to a command's return value.
///
/// A command body is one call to this, so no command hand-rolls the three-way split and no
/// second place can decide what an unexpected failure looks like on a wire.
///
/// No `Serialize` bounds: performing the mapping needs none, and Tauri already requires them at
/// each command signature. Stating a requirement once, where it is true, keeps this function
/// total.
pub fn resolve<T, E>(outcome: OpResult<T, E>) -> Result<Envelope<T, E>, Rejection> {
    match outcome {
        Ok(value) => Ok(Envelope::Value(value)),
        Err(Failure::Expected(error)) => Ok(Envelope::Refusal(error)),
        Err(Failure::Unexpected(unexpected)) => Err(Rejection::from(unexpected)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use serde_json::{Value, json};

    const SECRET: &str = "state file vanished mid-mutation: /Users/x/secret";

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Payload {
        entry_count: u32,
    }

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase", tag = "reason")]
    enum Refused {
        NoPublishedRevision,
    }

    fn value_of(serializable: &impl Serialize) -> Value {
        serde_json::to_value(serializable).unwrap()
    }

    #[test]
    fn a_successful_outcome_serializes_with_ok_true_and_its_value() {
        let outcome: OpResult<Payload, Refused> = Ok(Payload { entry_count: 2 });

        let response = resolve(outcome).unwrap();

        assert_eq!(
            value_of(&response),
            json!({ "ok": true, "value": { "entryCount": 2 } })
        );
    }

    #[test]
    fn an_expected_refusal_serializes_with_ok_false_and_its_error() {
        let outcome: OpResult<Payload, Refused> =
            Err(Failure::Expected(Refused::NoPublishedRevision));

        let response = resolve(outcome).unwrap();

        assert_eq!(
            value_of(&response),
            json!({ "ok": false, "error": { "reason": "noPublishedRevision" } })
        );
    }

    #[test]
    fn a_void_success_serializes_its_value_as_null_and_keeps_the_key() {
        // The hazard the design names is the *key disappearing*, not the value being wrong: a
        // property whose value is JavaScript `undefined` vanishes during JSON encoding, and a
        // decoder requiring "exactly the corresponding value or error property" would then
        // reject a success. So the key's presence is asserted separately from its value.
        let envelope: Envelope<(), Refused> = Envelope::Value(());

        let encoded = value_of(&envelope);

        assert_eq!(encoded, json!({ "ok": true, "value": null }));
        assert!(encoded.as_object().unwrap().contains_key("value"));
    }

    #[test]
    fn the_envelope_writes_its_discriminant_before_its_payload() {
        // Asserted on the string rather than the value, because key order is invisible to
        // `serde_json::Value` and a cross-language fixture diff is not invisible to it.
        let envelope: Envelope<u32, Refused> = Envelope::Value(1);
        assert_eq!(
            serde_json::to_string(&envelope).unwrap(),
            r#"{"ok":true,"value":1}"#
        );
    }

    #[test]
    fn a_rejection_built_from_an_unexpected_carries_only_its_correlation_identifier() {
        let unexpected = Unexpected::new(SECRET);

        // The negative control, and it belongs before the conversion: it proves the hazard is
        // real — the derived `Debug` does render the private message — and therefore that the
        // assertion below is not passing against a message that was never there.
        assert!(format!("{unexpected:?}").contains("secret"));
        assert!(unexpected.log_detail().contains("secret"));

        let correlation = unexpected.correlation();
        let rejection = Rejection::from(unexpected);

        // Exact equality, not `!contains("secret")`: a substring check is satisfied by any
        // unrelated shape, and is flaky the moment a message happens to be hex.
        assert_eq!(
            value_of(&rejection),
            json!({ "kind": "unexpected", "correlationId": correlation.to_string() })
        );
    }

    #[test]
    fn an_expected_refusal_never_reaches_the_rejection_channel() {
        // The negative control for the test above's *premise*. Without it, a green redaction
        // assertion could mean nothing ever takes the rejection path at all.
        let expected: OpResult<(), Refused> = Err(Failure::Expected(Refused::NoPublishedRevision));
        let unexpected: OpResult<(), Refused> = Err(Unexpected::new(SECRET).into());

        assert!(matches!(resolve(expected), Ok(Envelope::Refusal(_))));
        assert!(resolve(unexpected).is_err());
    }

    proptest! {
        #[test]
        fn no_message_can_influence_a_serialized_rejection(message: String) {
            // The total form of the claim the test above makes for one message: a serialized
            // rejection is a function of the correlation identifier alone.
            let unexpected = Unexpected::new(message);
            let correlation = unexpected.correlation();

            let encoded = serde_json::to_value(Rejection::from(unexpected)).unwrap();

            prop_assert_eq!(
                encoded,
                json!({ "kind": "unexpected", "correlationId": correlation.to_string() })
            );
        }
    }
}
