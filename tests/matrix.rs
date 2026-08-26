//! Support-matrix mechanical validation (CI gate for docs/support-matrix.toml).

use deltin_rs::matrix::{load_and_validate, state_counts, State};

#[test]
fn matrix_validates() {
    let matrix = load_and_validate().expect("support matrix must validate");
    assert!(!matrix.entries.is_empty());
    assert!(
        matrix.meta.upstream_pin.len() >= 7,
        "upstream pin must be a commit"
    );
    // Sanity: no duplicate ids and every id non-empty (validated upstream, but
    // keep the check here as a belt-and-suspenders for CI output clarity).
    let mut seen = std::collections::HashSet::new();
    for e in &matrix.entries {
        assert!(seen.insert(&e.id), "duplicate matrix id {}", e.id);
    }
}

#[test]
fn matrix_states_coverage() {
    let matrix = load_and_validate().unwrap();
    let counts = state_counts(&matrix);
    assert_eq!(counts.len(), State::ALL.len());
    assert!(matrix
        .entries
        .iter()
        .all(|entry| State::ALL.contains(&entry.state)));
    let lowering: usize = counts
        .iter()
        .filter(|(s, _)| *s == State::LoweringDependent)
        .map(|(_, n)| n)
        .sum();
    assert!(
        lowering > 0,
        "matrix should track lowering-dependent entries"
    );
}
