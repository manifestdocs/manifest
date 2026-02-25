//! OpenAPI spec coverage test.
//!
//! Ensures every route registered in the Axum router has a corresponding
//! entry in openapi.yaml, and vice versa. Catches spec drift at test time.
//!
//! How it works:
//! - Parses `.route("path", method(...))` calls from `src/api/mod.rs`
//! - Parses path + method entries from `openapi.yaml`
//! - Asserts the two sets are identical

use std::collections::BTreeSet;
use std::fs;

type Route = (String, String);

/// Extract registered routes from src/api/mod.rs by splitting on `.route(` calls.
fn routes_from_source() -> BTreeSet<Route> {
    let source = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/api/mod.rs"))
        .expect("could not read src/api/mod.rs");

    let mut routes = BTreeSet::new();

    // Each .route("path", method(handler)) call becomes a chunk after splitting
    for chunk in source.split(".route(").skip(1) {
        // Path is the first quoted string
        let Some(q1) = chunk.find('"') else { continue };
        let rest = &chunk[q1 + 1..];
        let Some(q2) = rest.find('"') else { continue };
        let path = rest[..q2].to_string();

        // HTTP methods appear as get(, post(, put(, delete( in this chunk
        for method in ["get", "post", "put", "delete"] {
            if chunk.contains(&format!("{method}(")) {
                routes.insert((path.clone(), method.to_uppercase()));
            }
        }
    }

    routes
}

/// Extract documented routes from openapi.yaml using indentation structure.
///
/// OpenAPI YAML uses consistent indentation:
/// - Path entries at 2-space indent: `  /projects/{id}:`
/// - Method entries at 4-space indent: `    get:`
fn routes_from_spec() -> BTreeSet<Route> {
    let spec = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/openapi.yaml"))
        .expect("could not read openapi.yaml");

    let mut routes = BTreeSet::new();
    let mut current_path = String::new();

    for line in spec.lines() {
        // Path entries: exactly 2-space indent, starts with /
        if line.starts_with("  /") && !line.starts_with("   ") {
            current_path = line.trim().trim_end_matches(':').to_string();
            continue;
        }

        // Method entries: exactly 4-space indent, known HTTP method
        if line.starts_with("    ") && !line.starts_with("     ") {
            let method = line.trim().trim_end_matches(':');
            if ["get", "post", "put", "delete", "patch"].contains(&method) {
                routes.insert((current_path.clone(), method.to_uppercase()));
            }
        }
    }

    routes
}

#[test]
fn openapi_spec_covers_all_routes() {
    let source_routes = routes_from_source();
    let spec_routes = routes_from_spec();

    let missing_from_spec: Vec<_> = source_routes.difference(&spec_routes).collect();
    let phantom_in_spec: Vec<_> = spec_routes.difference(&source_routes).collect();

    let mut failures = Vec::new();

    if !missing_from_spec.is_empty() {
        let mut msg = "Routes in mod.rs but MISSING from openapi.yaml:\n".to_string();
        for (path, method) in &missing_from_spec {
            msg.push_str(&format!("  {method} {path}\n"));
        }
        failures.push(msg);
    }

    if !phantom_in_spec.is_empty() {
        let mut msg = "Routes in openapi.yaml but NOT in mod.rs:\n".to_string();
        for (path, method) in &phantom_in_spec {
            msg.push_str(&format!("  {method} {path}\n"));
        }
        failures.push(msg);
    }

    if !failures.is_empty() {
        panic!(
            "OpenAPI spec out of sync!\n\n{}\nmod.rs routes: {}\nopenapi.yaml routes: {}\n",
            failures.join("\n"),
            source_routes.len(),
            spec_routes.len(),
        );
    }

    // Sanity: verify the parsers found a reasonable number of routes
    assert!(
        source_routes.len() > 30,
        "Expected 30+ routes, found {}. Parser may be broken.",
        source_routes.len(),
    );
}
