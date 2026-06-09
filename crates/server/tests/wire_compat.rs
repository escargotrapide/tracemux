//! Wire-protocol v1 compatibility fixtures.
//!
//! REQ: FR-WIRE-001 (frozen v0.1 schema)
//!
//! Fixtures live under [`tests/compat/wire/v1/`] (workspace-relative).
//! Each fixture is the **byte-exact** MessagePack payload of a known
//! [`tracemux_server::wire::Envelope`].
//!
//! On a normal run the test:
//!
//! 1. constructs the canonical envelope,
//! 2. encodes it,
//! 3. compares against the on-disk fixture byte-for-byte,
//! 4. decodes the on-disk fixture and asserts the envelope round-trips.
//!
//! If `TRACEMUX_WIRE_BLESS=1` is set, missing or stale fixtures are
//! (re)written. **Never bless on CI** -- re-blessing is the same as
//! changing the wire schema and requires an ADR + subprotocol bump
//! (see `docs/protocols/wire-protocol.md`).

use std::fs;
use std::path::PathBuf;

use rmpv::Value;
use tracemux_server::wire::{decode, encode, Envelope, FrameType};

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

    if std::env::var_os("TRACEMUX_WIRE_BLESS").is_some() || !path.exists() {
        fs::write(&path, &encoded).expect("write fixture");
        eprintln!("wire-compat: wrote {}", path.display());
    } else {
        let on_disk = fs::read(&path).expect("read fixture");
        assert_eq!(
            on_disk, encoded,
            "fixture {name} drifted. \
             If this is intentional, add an ADR + bump subprotocol \
             token, then run with TRACEMUX_WIRE_BLESS=1."
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
            Value::String("tracemux-test".into()),
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

// REQ: FR-WIRE-001
// REQ: FR-SINK-WIRE
#[test]
fn fixture_write_bytes() {
    let payload = Value::Map(vec![(
        Value::String("body".into()),
        Value::Binary(b"hello".to_vec()),
    )]);
    let env = Envelope::new(FrameType::Write, 7, payload)
        .with_sid("00000000-0000-4000-8000-000000000001")
        .with_ch(0);
    check("write_bytes", &env);
}

// REQ: FR-WIRE-001
// REQ: FR-WIRE-003
#[test]
fn fixture_sources_with_decoder_metadata() {
    let row = Value::Map(vec![
        (
            Value::String("sid".into()),
            Value::String("00000000-0000-4000-8000-000000000001".into()),
        ),
        (Value::String("name".into()), Value::String("COM7".into())),
        (Value::String("kind".into()), Value::String("serial".into())),
        (
            Value::String("status".into()),
            Value::String("running".into()),
        ),
        (
            Value::String("channels".into()),
            Value::Array(vec![Value::from(0_u64)]),
        ),
        (Value::String("bytes_in".into()), Value::from(12_u64)),
        (Value::String("persistent".into()), Value::Boolean(true)),
        (
            Value::String("decoder".into()),
            Value::String("utf8-text:shift_jis".into()),
        ),
        (
            Value::String("encoding".into()),
            Value::String("shift_jis".into()),
        ),
    ]);
    let payload = Value::Map(vec![
        (
            Value::String("event".into()),
            Value::String("sources".into()),
        ),
        (
            Value::String("message".into()),
            Value::String("sources listed".into()),
        ),
        (Value::String("sources".into()), Value::Array(vec![row])),
    ]);
    let env = Envelope::new(FrameType::Ctl, 8, payload);
    check("sources_with_decoder_metadata", &env);
}

// REQ: FR-WIRE-001
// REQ: FR-WIRE-003
#[test]
fn fixture_sources_with_detection_metadata() {
    let detection = Value::Map(vec![
        (Value::String("mode".into()), Value::String("auto".into())),
        (Value::String("sample_bytes".into()), Value::from(32_u64)),
        (
            Value::String("configured_encoding".into()),
            Value::String("utf-8".into()),
        ),
        (
            Value::String("effective_encoding".into()),
            Value::String("shift_jis".into()),
        ),
        (
            Value::String("sampled_encoding".into()),
            Value::String("shift_jis".into()),
        ),
        (
            Value::String("encoding_candidates".into()),
            Value::Array(vec![Value::Map(vec![
                (
                    Value::String("label".into()),
                    Value::String("shift_jis".into()),
                ),
                (Value::String("confidence".into()), Value::from(96_u64)),
                (Value::String("had_errors".into()), Value::Boolean(false)),
                (
                    Value::String("evidence".into()),
                    Value::Array(vec![Value::String("decode-clean".into())]),
                ),
            ])]),
        ),
        (
            Value::String("log_type_candidates".into()),
            Value::Array(vec![Value::Map(vec![
                (
                    Value::String("tag".into()),
                    Value::String("error-id".into()),
                ),
                (Value::String("kind".into()), Value::String("regex".into())),
                (
                    Value::String("pattern".into()),
                    Value::String("E-[0-9]{4}".into()),
                ),
                (Value::String("count".into()), Value::from(1_u64)),
                (Value::String("confidence".into()), Value::from(70_u64)),
            ])]),
        ),
    ]);
    let row = Value::Map(vec![
        (
            Value::String("sid".into()),
            Value::String("00000000-0000-4000-8000-000000000001".into()),
        ),
        (Value::String("name".into()), Value::String("COM7".into())),
        (Value::String("kind".into()), Value::String("serial".into())),
        (
            Value::String("status".into()),
            Value::String("running".into()),
        ),
        (
            Value::String("channels".into()),
            Value::Array(vec![Value::from(0_u64)]),
        ),
        (Value::String("bytes_in".into()), Value::from(12_u64)),
        (Value::String("persistent".into()), Value::Boolean(true)),
        (
            Value::String("decoder".into()),
            Value::String("utf8-text:shift_jis".into()),
        ),
        (
            Value::String("encoding".into()),
            Value::String("shift_jis".into()),
        ),
        (
            Value::String("detection_mode".into()),
            Value::String("auto".into()),
        ),
        (Value::String("detection".into()), detection),
    ]);
    let payload = Value::Map(vec![
        (
            Value::String("event".into()),
            Value::String("sources".into()),
        ),
        (
            Value::String("message".into()),
            Value::String("sources listed".into()),
        ),
        (Value::String("sources".into()), Value::Array(vec![row])),
    ]);
    let env = Envelope::new(FrameType::Ctl, 9, payload);
    check("sources_with_detection_metadata", &env);
}

// REQ: FR-WIRE-001
// REQ: FR-SINK-WIRE
// ADR-0004: additive resize-only write frame (no body).
#[test]
fn fixture_write_resize_only() {
    let payload = Value::Map(vec![(
        Value::String("resize".into()),
        Value::Map(vec![
            (Value::String("cols".into()), Value::from(120_u64)),
            (Value::String("rows".into()), Value::from(40_u64)),
        ]),
    )]);
    let env = Envelope::new(FrameType::Write, 11, payload)
        .with_sid("00000000-0000-4000-8000-000000000001")
        .with_ch(0);
    check("write_resize_only", &env);
}

// REQ: FR-WIRE-001
// REQ: FR-WIRE-003
// ADR-0005: additive local_echo_default / newline_default on sources rows.
#[test]
fn fixture_sources_with_terminal_input_defaults() {
    let row = Value::Map(vec![
        (
            Value::String("sid".into()),
            Value::String("00000000-0000-4000-8000-000000000001".into()),
        ),
        (
            Value::String("name".into()),
            Value::String("cmd.exe".into()),
        ),
        (
            Value::String("kind".into()),
            Value::String("process".into()),
        ),
        (
            Value::String("status".into()),
            Value::String("running".into()),
        ),
        (
            Value::String("channels".into()),
            Value::Array(vec![Value::from(0_u64)]),
        ),
        (Value::String("bytes_in".into()), Value::from(12_u64)),
        (Value::String("persistent".into()), Value::Boolean(true)),
        (
            Value::String("local_echo_default".into()),
            Value::String("on".into()),
        ),
        (
            Value::String("newline_default".into()),
            Value::String("crlf".into()),
        ),
    ]);
    let payload = Value::Map(vec![
        (
            Value::String("event".into()),
            Value::String("sources".into()),
        ),
        (
            Value::String("message".into()),
            Value::String("sources listed".into()),
        ),
        (Value::String("sources".into()), Value::Array(vec![row])),
    ]);
    let env = Envelope::new(FrameType::Ctl, 12, payload);
    check("sources_with_terminal_input_defaults", &env);
}
