//! Publishes the skeleton revision into an application-data directory, without running the app.
//!
//! # Why this exists
//!
//! Phase 3's thread is candidate → bundle → atomic publication → Tauri read → React page, and
//! STE-19 deliberately gives the frontend no build trigger: `@tauri-apps/api` stays confined to the
//! documentation adapter until Phase 10 introduces the desktop control module. So the window can
//! exercise the read, but something has to publish first, and this is that something.
//!
//! It reuses [`open_stores`](stellaris_docs_lib::composition::open_stores) and
//! [`DocumentationHost`] rather than reimplementing any of it — `open_stores` is deliberately free
//! of Tauri precisely so it can be driven without an `App`.
//!
//! # Preconditions the caller owns
//!
//! **Run this with the app closed.** D-065 gives one process sole ownership of an
//! application-data directory, and `tauri-plugin-single-instance` cannot protect a directory
//! against a differently-identified binary such as this one — "this isolation is a caller
//! precondition, not a guarantee supplied by the plugin".
//!
//! The directory is the one a `test-support` launch prints on startup.
//!
//! Behind `test-support`, so it does not exist in a shipped build: it publishes a candidate that
//! documents nothing real, which is exactly what a release binary must have no path to.
//!
//! # Why an example rather than a second `[[bin]]`
//!
//! The Tauri CLI picks *an* executable target as "the application" to bundle, and with two `[[bin]]`
//! targets it chose this one — `tauri build` reported it as the built application, which in Phase 12
//! would have meant packaging a seeding utility as the app. Cargo examples are not bin targets, so
//! the ambiguity does not arise, and `cargo clippy --all-targets` and `cargo test` still compile
//! this file.
//!
//! ```text
//! cargo run --example seed-skeleton --features test-support -- <application-data-directory>
//! ```

#[cfg(not(feature = "test-support"))]
fn main() {
    eprintln!("seed-skeleton requires the `test-support` feature");
    std::process::exit(2);
}

#[cfg(feature = "test-support")]
fn main() -> std::process::ExitCode {
    use std::path::PathBuf;
    use std::sync::Arc;

    use stellaris_docs_lib::application::DocumentationHost;
    use stellaris_docs_lib::composition::open_stores;
    use stellaris_docs_lib::testsupport::candidates::seed_skeleton_thread;

    let Some(app_data) = std::env::args().nth(1).map(PathBuf::from) else {
        eprintln!(
            "usage: seed-skeleton <application-data-directory>\n\
             The directory is printed on startup by a `test-support` build of the app."
        );
        return std::process::ExitCode::from(2);
    };

    let stores = match open_stores(&app_data) {
        Ok(stores) => stores,
        Err(refusal) => {
            eprintln!("cannot open {}: {refusal}", app_data.display());
            return std::process::ExitCode::FAILURE;
        }
    };

    // The same seeding the app performs on launch, and idempotent for the same reason: it looks up
    // the marker Discovery Location rather than adding one, so running this and then launching the
    // app does not mint a second location and a second installation identifier.
    let (target, source) = seed_skeleton_thread(&stores.state);
    let host = Arc::new(DocumentationHost::new(
        stores.state,
        stores.revisions,
        Box::new(source),
    ));

    match host.publish_documentation(&target) {
        Ok(published) => {
            println!(
                "published for installation {} (durable: {})",
                target.installation(),
                published.durability.is_fully_confirmed(),
            );
            std::process::ExitCode::SUCCESS
        }
        Err(failure) => {
            // `Display` on the expected half is the redacted form; the unexpected half prints its
            // correlation identifier and nothing else, same as a transport would.
            eprintln!("could not publish: {failure:?}");
            std::process::ExitCode::FAILURE
        }
    }
}
