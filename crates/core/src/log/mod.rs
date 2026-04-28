//! On-disk session-dir format. **WAL / group-commit / rotate are
//! critical paths.** See `docs/protocols/log-format.md`.

pub mod clock_table;
pub mod frames;
pub mod group_commit;
pub mod index;
pub mod lines;
pub mod raw;
pub mod retention;
pub mod rotate;
pub mod schemas;
pub mod wal;
