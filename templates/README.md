# Code templates

These templates are copied (manually or via `cargo generate`) when
adding new pipeline components. Each template contains:

- a Rust source file with a minimal `Source` / `Framer` / `Decoder` /
  `LogSink` / `Importer` / `Exporter` impl;
- a TOML fragment showing the new `ChannelSpec` variant;
- a TODO list mapping to the matching skill in `.github/skills/`.

| Template          | Skill                          |
| ----------------- | ------------------------------ |
| `source/`         | `add-source/SKILL.md`          |
| `sink/`           | `add-sink/SKILL.md`            |
| `framer/`         | `add-framer/SKILL.md`          |
| `decoder/`        | `add-decoder/SKILL.md`         |
| `importer/`       | `add-importer/SKILL.md`        |
| `panel/`          | `add-ui-panel/SKILL.md`        |
