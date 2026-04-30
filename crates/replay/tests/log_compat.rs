//! Log-format v1 compatibility fixtures.
//!
//! REQ: FR-LOG-001

use std::path::{Path, PathBuf};

use wanlogger_core::log::index::{Dir, IndexEntry, Kind};
use wanlogger_core::log::raw::RawReader;
use wanlogger_core::time::{ClockQuality, ClockSource};

fn workspace_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // crates/replay -> crates
    p.pop(); // crates -> workspace root
    p
}

fn fixture_dir() -> PathBuf {
    workspace_root()
        .join("tests")
        .join("compat")
        .join("log")
        .join("v1")
        .join("minimal-session")
}

fn read_index(path: &Path) -> Vec<IndexEntry> {
    std::fs::read_to_string(path.join("index.jsonl"))
        .expect("read index fixture")
        .lines()
        .map(|line| serde_json::from_str(line).expect("parse index fixture line"))
        .collect()
}

// REQ: FR-LOG-001
#[test]
fn minimal_session_fixture_has_stable_index_shape_and_offsets() {
    let dir = fixture_dir();
    let entries = read_index(&dir);
    assert_eq!(entries.len(), 2);

    let first = &entries[0];
    assert_eq!(first.ts_origin, "2023-11-14T22:13:20Z");
    assert_eq!(first.ts_ingest, "2023-11-14T22:13:20.000500Z");
    assert_eq!(first.mono_ns, 1000);
    assert_eq!(first.clock_quality, ClockQuality::Imported);
    assert_eq!(first.clock_source, ClockSource::Imported);
    assert_eq!(first.dir, Dir::In);
    assert_eq!(first.kind, Kind::Bytes);
    assert_eq!(first.off, 0);
    assert_eq!(first.len, 5);
    assert_eq!(first.source.as_deref(), Some("compat:minimal"));
    assert_eq!(first.tags, ["compat", "v1"]);

    let second = &entries[1];
    assert_eq!(second.mono_ns, 2000);
    assert_eq!(second.off, 5);
    assert_eq!(second.len, 4);

    let mut raw = RawReader::open(&dir).expect("open raw fixture");
    assert_eq!(raw.read_at(first.off, first.len).unwrap(), b"alpha");
    assert_eq!(raw.read_at(second.off, second.len).unwrap(), b"beta");
}

// REQ: FR-LOG-001
#[tokio::test]
async fn replay_consumes_minimal_session_fixture() {
    let stats = wanlogger_replay::run(&fixture_dir(), 0.0, Some(0))
        .await
        .expect("replay compat fixture");
    assert_eq!(stats.records, 2);
}
