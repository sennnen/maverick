//! Holds docs/errors.md and the code registry in mav-model in mechanical step. Both surveyed
//! codebases had prose that drifted from their code; this is one of the checks that stops that
//! happening here.
// Tests are allowed to panic; the workspace-level denies apply to library code.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use mav_model::error::codes;
use std::collections::BTreeMap;
use std::path::PathBuf;

fn documented_codes() -> BTreeMap<u16, String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../docs/errors.md");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));

    let mut found = BTreeMap::new();
    for line in text.lines() {
        let cells: Vec<&str> = line.split('|').map(str::trim).collect();
        // A catalogue row looks like: | 2001 | FRAME_HEADER_CRC_MISMATCH | ... |
        if cells.len() >= 4 {
            if let Ok(code) = cells[1].parse::<u16>() {
                let previous = found.insert(code, cells[2].to_owned());
                assert!(
                    previous.is_none(),
                    "code {code} documented twice in errors.md"
                );
            }
        }
    }
    found
}

#[test]
fn errors_md_catalogue_matches_the_code_registry() {
    let documented = documented_codes();
    assert!(
        !documented.is_empty(),
        "no catalogue rows found in docs/errors.md"
    );

    for &(code, name) in codes::ALL {
        match documented.get(&code) {
            None => panic!("code {code} ({name}) is in codes::ALL but not in docs/errors.md"),
            Some(doc_name) => assert_eq!(
                doc_name, name,
                "code {code} is named {name} in code but {doc_name} in docs/errors.md"
            ),
        }
    }
    for (code, name) in &documented {
        assert!(
            codes::ALL.iter().any(|&(c, _)| c == *code),
            "code {code} ({name}) is documented in errors.md but missing from codes::ALL"
        );
    }
}
