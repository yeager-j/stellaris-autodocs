//! The baseline desktop Content Security Policy, enforced as a rule rather than described as one.
//!
//! `docs/technical-design.md`, "Companion same-origin policy" states the gate in prose: policies
//! "name only the minimum packaged or same-origin sources", and "remote script, stylesheet, font,
//! and image origins and `unsafe-eval` are forbidden". Prose is not a control, so this module
//! reads the shipped configuration and checks it.
//!
//! # What this gate does not prove
//!
//! **That any webview enforced anything.** It proves the string in `tauri.conf.json` satisfies the
//! baseline. Whether the packaged application actually applies it is observed by running a built
//! artifact and reading the response header, and the release gate against production artifacts is
//! Phase 12 (docs/implementation-plan.md, Phase 12 task 2). Confusing this test for that one would
//! be exactly the "gate that has always been green" the project's diagnostics warn about.
//!
//! # Why the rules are a function over a string
//!
//! So they can be aimed at a policy that is deliberately wrong. A gate that can only ever read the
//! repository's own configuration cannot be shown to detect anything, and
//! [`each_baseline_rule_detects_its_own_violation`] is what makes the green runs mean something.

use serde_json::Value;

/// The configuration the application ships.
const CONFIG: &str = include_str!("../../tauri.conf.json");

/// The development identity overlay. `include_str!` rather than a runtime read so that editing
/// either file forces this gate to recompile and run again.
const DEV_OVERLAY: &str = include_str!("../../tauri.dev.conf.json");

/// Directives that must name exactly one source, and which.
///
/// `connect-src` is absent because it is the one directive with more than one legitimate source;
/// it gets its own rule below.
const EXACT: [(&str, &str); 11] = [
    ("default-src", "'self'"),
    ("script-src", "'self'"),
    ("style-src", "'self'"),
    ("font-src", "'self'"),
    ("img-src", "'self'"),
    ("worker-src", "'none'"),
    ("object-src", "'none'"),
    ("base-uri", "'none'"),
    ("frame-src", "'none'"),
    ("frame-ancestors", "'none'"),
    ("form-action", "'none'"),
];

/// The IPC endpoints `invoke` needs.
///
/// **Both spellings, on every platform.** Tauri's injected IPC script sets
/// `canUseCustomProtocol = osName !== 'android'`, so macOS and Linux `fetch` `ipc://localhost`
/// and Windows fetches `http://ipc.localhost`; `postMessage` is only the fallback taken *after*
/// that fetch fails. Omitting these does not visibly break macOS — it ships a silent CSP violation
/// and a wasted round trip per call, which is worse than breaking.
const IPC_ENDPOINTS: [&str; 2] = ["ipc:", "http://ipc.localhost"];

/// Every source token the baseline permits, anywhere.
///
/// An allow-list rather than a list of forbidden schemes, because the forbidden list is the one
/// that is wrong the day CSP grows a source expression nobody here has heard of. The negative
/// control found this the hard way: `https:` is a bare scheme rather than an absolute URL, so a
/// deny-list keyed on `http://`-style prefixes let it through `connect-src`, which has no
/// "exactly these sources" rule to catch it.
const PERMITTED_SOURCES: [&str; 4] = ["'self'", "'none'", "ipc:", "http://ipc.localhost"];

/// Every way `csp` fails the baseline, named. Empty means it passes.
fn violations(csp: &str) -> Vec<String> {
    let directives: Vec<(&str, Vec<&str>)> = csp
        .split(';')
        .filter_map(|directive| {
            let mut tokens = directive.split_whitespace();
            let name = tokens.next()?;
            Some((name, tokens.collect()))
        })
        .collect();

    let mut failures = Vec::new();

    for (name, sources) in &directives {
        for source in sources {
            if PERMITTED_SOURCES.contains(source) {
                continue;
            }
            // The specific diagnoses come first, because "unsafe-eval" tells a reader what they
            // did in a way "a source the baseline does not permit" does not.
            if *source == "'unsafe-inline'" {
                failures.push(format!("{name} allows unsafe-inline"));
            } else if *source == "'unsafe-eval'" {
                failures.push(format!("{name} allows unsafe-eval"));
            } else if source.contains('*') {
                failures.push(format!("{name} names a wildcard source: {source}"));
            } else {
                failures.push(format!("{name} names a remote source: {source}"));
            }
        }
        // Style attributes are governed separately from stylesheets, and the design permits
        // `style-src-attr 'unsafe-inline'` only for a diagram renderer that demands it, documented
        // and tested. Nothing has demanded it, so its presence is a regression.
        if *name == "style-src-attr" {
            failures.push("style-src-attr is set, which no component has justified".to_owned());
        }
    }

    for (name, expected) in EXACT {
        match directives.iter().find(|(found, _)| *found == name) {
            None => failures.push(format!("{name} is missing")),
            Some((_, sources)) if sources.as_slice() != [expected] => failures.push(format!(
                "{name} should be exactly {expected}, found {}",
                sources.join(" ")
            )),
            Some(_) => {}
        }
    }

    match directives.iter().find(|(name, _)| *name == "connect-src") {
        None => failures.push("connect-src is missing".to_owned()),
        Some((_, sources)) => {
            if !sources.contains(&"'self'") {
                failures.push("connect-src does not allow 'self'".to_owned());
            }
            for endpoint in IPC_ENDPOINTS {
                if !sources.contains(&endpoint) {
                    failures.push(format!(
                        "connect-src is missing the IPC endpoint {endpoint}"
                    ));
                }
            }
        }
    }

    failures
}

fn config() -> Value {
    serde_json::from_str(CONFIG).expect("tauri.conf.json is valid JSON")
}

fn configured_csp() -> String {
    config()["app"]["security"]["csp"]
        .as_str()
        .expect("a Content Security Policy is configured, as a string")
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_configured_policy_satisfies_every_baseline_rule() {
        assert_eq!(violations(&configured_csp()), Vec::<String>::new());
    }

    #[test]
    fn the_policy_is_one_string_rather_than_a_directive_map() {
        // Tauri accepts either. The map form would silently pass every rule above, because
        // `configured_csp` would panic and no directive would ever be examined.
        assert!(config()["app"]["security"]["csp"].is_string());
    }

    #[test]
    fn no_development_policy_is_configured() {
        // `AppManager::csp()` prefers `devCsp` in development, but the value is only ever consumed
        // by `get_asset`, which serves the *embedded* assets. `build.devUrl` points at the Vite
        // dev server, so the webview loads `http://localhost:1420` and Tauri injects nothing at
        // all. A `devCsp` key would therefore be a configuration entry that cannot take effect —
        // a control that never acts, which is worse than its absence because it reads as one.
        assert!(
            config()["app"]["security"].get("devCsp").is_none(),
            "a devCsp can never apply while build.devUrl names a dev server; \
             `npm run app:verify` is the loop that exercises the real policy"
        );
    }

    #[test]
    fn the_development_overlay_changes_only_the_application_identity() {
        let overlay: Value =
            serde_json::from_str(DEV_OVERLAY).expect("tauri.dev.conf.json is valid JSON");

        let identifier = overlay["identifier"]
            .as_str()
            .expect("the overlay sets an identifier");
        assert_ne!(
            identifier,
            config()["identifier"].as_str().unwrap(),
            "the overlay exists to give development its own application-data directory and \
             single-instance key; an identical identifier silently shares both"
        );

        // The whole key set, not a deny-list of the keys that happen to be dangerous today.
        // "Changes only the application identity" is the invariant, and naming `app` alone would
        // stay green when someone adds `build`, `bundle`, or `plugins` — each of which silently
        // reconfigures the development binary away from what the production config describes.
        //
        // `app` is merely the worst case: config merge is RFC 7386, which replaces arrays
        // wholesale, so an `app.windows` entry would discard the base window's title and size
        // rather than adding to them, and an `app.security` entry could weaken the policy this
        // module exists to enforce.
        let mut keys: Vec<&str> = overlay
            .as_object()
            .expect("the overlay is a JSON object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            ["$schema", "identifier"],
            "the development overlay must carry the application identity and nothing else"
        );
    }

    #[test]
    fn each_baseline_rule_detects_its_own_violation() {
        // The negative control. Every row is the shipped policy with exactly one directive
        // mutated, so a rule that stopped examining anything fails here rather than passing
        // quietly above.
        let rows = [
            (
                "default-src 'self'",
                "default-src 'self' https:",
                "remote source",
            ),
            (
                "script-src 'self'",
                "script-src 'self' 'unsafe-eval'",
                "unsafe-eval",
            ),
            (
                "style-src 'self'",
                "style-src 'self' 'unsafe-inline'",
                "unsafe-inline",
            ),
            (
                "img-src 'self'",
                "img-src 'self' https://example.invalid",
                "remote source",
            ),
            ("font-src 'self'", "font-src *", "wildcard"),
            (
                "frame-ancestors 'none'",
                "frame-ancestors 'self'",
                "frame-ancestors should be exactly",
            ),
            (
                "object-src 'none'",
                "object-src 'self'",
                "object-src should be exactly",
            ),
            (
                "connect-src 'self' ipc: http://ipc.localhost",
                "connect-src 'self' ipc:",
                "missing the IPC endpoint http://ipc.localhost",
            ),
            (
                "connect-src 'self' ipc: http://ipc.localhost",
                "connect-src 'self' http://ipc.localhost",
                "missing the IPC endpoint ipc:",
            ),
            (
                "connect-src 'self' ipc: http://ipc.localhost",
                "style-src-attr 'unsafe-inline'",
                "style-src-attr is set",
            ),
            // The regression this table already caught once: `https:` is a bare scheme, not an
            // absolute URL, and `connect-src` has no "exactly these sources" rule to fall back on.
            // A deny-list keyed on `http://`-style prefixes passed this row; the allow-list does
            // not. Reintroduce the deny-list and this is the row that goes red.
            (
                "connect-src 'self' ipc: http://ipc.localhost",
                "connect-src 'self' ipc: http://ipc.localhost https:",
                "connect-src names a remote source: https:",
            ),
        ];

        let policy = configured_csp();
        for (original, mutated, expected) in rows {
            assert!(
                policy.contains(original),
                "the negative control mutates `{original}`, which the shipped policy no longer \
                 contains — update the control rather than deleting it"
            );
            let broken = policy.replace(original, mutated);

            let reported = violations(&broken);
            assert!(
                reported.iter().any(|failure| failure.contains(expected)),
                "mutating `{original}` into `{mutated}` should report `{expected}`, reported {reported:?}"
            );
        }
    }

    #[test]
    fn a_missing_directive_is_a_violation_rather_than_an_absence() {
        // Deleting a rule's directive entirely must fail too. Without this, a policy could satisfy
        // every "exactly" check by naming nothing at all.
        let policy = configured_csp();
        let without_object_src = policy.replace("; object-src 'none'", "");

        assert!(
            violations(&without_object_src)
                .iter()
                .any(|failure| failure == "object-src is missing")
        );
    }
}
