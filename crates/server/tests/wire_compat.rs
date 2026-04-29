//! Wire-protocol v1 compatibility fixtures.
//!
//! REQ: FR-WIRE-001 (frozen v0.1 schema)
//!
//! Fixtures live under [`tests/compat/wire/v1/`] (workspace-relative).
//! Each fixture is the **byte-exact** MessagePack payload of a known
//! [`wanlogger_server::wire::Envelope`].
//!
//! On a normal run the test:
//!
//! 1. constructs the canonical envelope,
//! 2. encodes it,
//! 3. compares against the on-disk fixture byte-for-byte,
//! 4. decodes the on-disk fixture and asserts the envelope round-trips.
//!
//! If `WANLOGGER_WIRE_BLESS=1` is set, missing or stale fixtures are
//! (re)written. **Never bless on CI** ? re-blessing is the same as
//! changing the wire schema and requires an ADR + subprotocol bump
//! (see `docs/protocols/wire-protocol.md`).

use std::fs;
use std::path::PathBuf;

use rmpv::Value;
use wanlogger_server::wire::{decode, encode, Envelope, FrameType};

fn fixture_dir() -> PathBuf {
    // Walk up from this file (`crates/server/tests/...`) to the
    // workspace root, then dive into `tests/compat/wire/v1`.
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // crates/server -> crates
    p.pop(); // crates -> workspace root
    p.push("tests");
    p.push("compat");
    p.push("wire");
    p.push("v1");
    p
}

fn check(name: &str, env: &Envelope) {
    let dir = fixture_dir();
    fs::create_dir_all(&dir).expect("create fixture dir");
    let path = dir.join(format!("{name}.msgpack"));
    let encoded = encode(env).expect("encode");

    if std::env::var_os("WANLOGGER_WIRE_BLESS").is_some() || !path.exists() {
        fs::write(&path, &encoded).expect("write fixture");
        eprintln!("wire-compat: wrote {}", path.display());
    } else {
        let on_disk = fs::read(&path).expect("read fixture");
        assert_eq!(
            on_disk, encoded,
            "fixture {name} drifted. \
             If this is intentional, add an ADR + bump subprotocol \
             token, then run with WANLOGGER_WIRE_BLESS=1."
        );
    }

    let on_disk = fs::read(&path).expect("read fixture");
    let back = decode(&on_disk).expect("decode fixture");
    assert_eq!(&back, env, "decoded fixture {name} mismatched envelope");
}

// REQ: FR-WIRE-001
#[test]
fn fixture_ping() {
    let env = Envelope::new(FrameType::Ping, 1, Value::Nil);
    check("ping", &env);
}

// REQ: FR-WIRE-001
#[test]
fn fixture_pong() {
    let env = Envelope::new(FrameType::Pong, 1, Value::Nil);
    check("pong", &env);
}

// REQ: FR-WIRE-001
#[test]
fn fixture_hello() {
    let payload = Value::Map(vec![
        (
            Value::String("app".into()),
            Value::String("wanlogger-test".into()),
        ),
        (
            Value::String("version".into()),
            Value::String("0.1.0".into()),
        ),
    ]);
    let env = Envelope::new(FrameType::Hello, 0, payload);
    check("hello", &env);
}

// REQ: FR-WIRE-001
#[test]
fn fixture_data_with_sid_ch() {
    let body = Value::Binary(b"hello".to_vec());
    let payload = Value::Map(vec![
        (Value::String("dir".into()), Value::String("in".into())),
        (Value::String("kind".into()), Value::String("bytes".into())),
        (Value::String("body".into()), body),
    ]);
    let env = Envelope::new(FrameType::Data, 42, payload)
        .with_sid("00000000-0000-4000-8000-000000000001")
        .with_ch(0);
    check("data_bytes", &env);
}
