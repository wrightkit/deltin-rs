use std::path::PathBuf;

use deltin_rs::{FileId, SourceBridgeError, SourceMap, Span, WorkshopSourceBridge};
use workshop_rs::source::{Position, SourceFile};

#[test]
fn bridge_preserves_cross_file_identity_and_unicode_positions() {
    let mut sources = SourceMap::new();
    let first = sources.add_file(PathBuf::from("src/main.del"), "rule Main {}".to_string());
    let second = sources.add_file(PathBuf::from("lib/β.del"), "α\n世界\n".to_string());

    let bridge = WorkshopSourceBridge::from_source_map(&sources).expect("source bridge");
    let first_workshop = bridge.workshop_file_id(first).expect("first file mapping");
    let second_workshop = bridge
        .workshop_file_id(second)
        .expect("second file mapping");

    assert_ne!(first_workshop, second_workshop);
    assert_eq!(bridge.files().len(), 2);
    assert_eq!(
        bridge.files().get(first_workshop),
        Some(&SourceFile::new("src/main.del"))
    );
    assert_eq!(
        bridge.files().get(second_workshop),
        Some(&SourceFile::new("lib/β.del"))
    );

    // `世界` occupies bytes 3..9, but columns 1..3: columns count Unicode
    // scalar values, not UTF-8 bytes.
    let workshop_span = bridge
        .span(Span::new(second, 3, 9))
        .expect("Unicode span should map exactly");
    assert_eq!(workshop_span.file, second_workshop);
    assert_eq!(workshop_span.start, Position::new(2, 1));
    assert_eq!(workshop_span.end, Position::new(2, 3));
    assert!(workshop_span.is_valid());
}

#[test]
fn bridge_maps_one_based_line_boundaries_and_eof() {
    let mut sources = SourceMap::new();
    let file = sources.add_file(PathBuf::from("boundary.del"), "éx\nlast".to_string());
    let bridge = WorkshopSourceBridge::from_source_map(&sources).expect("source bridge");

    assert_eq!(bridge.position(file, 0).unwrap(), Position::new(1, 1));
    assert_eq!(bridge.position(file, 2).unwrap(), Position::new(1, 2));
    assert_eq!(bridge.position(file, 3).unwrap(), Position::new(1, 3));
    assert_eq!(bridge.position(file, 4).unwrap(), Position::new(2, 1));
    assert_eq!(bridge.position(file, 8).unwrap(), Position::new(2, 5));
    assert_eq!(
        bridge.span(Span::new(file, 3, 8)).unwrap(),
        workshop_rs::source::Span::new(
            bridge.workshop_file_id(file).unwrap(),
            Position::new(1, 3),
            Position::new(2, 5),
        )
    );
}

#[test]
fn bridge_rejects_unknown_reversed_and_non_boundary_locations() {
    let mut sources = SourceMap::new();
    let file = sources.add_file(PathBuf::from("errors.del"), "é".to_string());
    let bridge = WorkshopSourceBridge::from_source_map(&sources).expect("source bridge");

    assert_eq!(
        bridge.position(FileId(99), 0),
        Err(SourceBridgeError::UnknownFile(FileId(99)))
    );
    assert_eq!(
        bridge.span(Span::new(file, 2, 1)),
        Err(SourceBridgeError::ReversedSpan(Span::new(file, 2, 1)))
    );
    assert_eq!(
        bridge.position(file, 1),
        Err(SourceBridgeError::OffsetNotCharBoundary { file, offset: 1 })
    );
    assert_eq!(
        bridge.position(file, 3),
        Err(SourceBridgeError::OffsetOutOfBounds {
            file,
            offset: 3,
            len: 2,
        })
    );
}
