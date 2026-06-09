//! Regression tests for shipped example TOML configs.

use std::fs;
use std::path::Path;

use tracemux_core::config::schema_v1::ConfigV1;

#[test]
fn shipped_example_configs_parse_as_v1() {
    let examples = [
        "mock.toml",
        "serial.toml",
        "tcp-listener.toml",
        "multi-source.toml",
        "packet-capture.toml",
        "windows-shell.toml",
    ];
    let examples_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");

    for example in examples {
        let path = examples_dir.join(example);
        let body = fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("reading {}: {err}", path.display()));
        let config: ConfigV1 =
            toml::from_str(&body).unwrap_or_else(|err| panic!("parsing {}: {err}", path.display()));

        assert_eq!(config.config_version, 1, "{example}");
        assert!(
            !config.channels.is_empty(),
            "{example} should define at least one channel"
        );
    }
}
