//! Public v0.1 API-surface checks.

#![allow(clippy::missing_panics_doc)]

use wanlogger_core::{
    decoder::Decoder, exporter::Exporter, framer::Framer, importer::Importer, logsink::LogSink,
    sink::Sink, source::Source, timeseries::TimeseriesSink, TimeSource,
};

fn assert_send<T: Send + ?Sized>() {}
fn assert_sync<T: Sync + ?Sized>() {}

// REQ: FR-CORE-001
#[test]
fn frozen_v0_1_traits_are_public_trait_objects() {
    assert_send::<dyn Source>();
    assert_sync::<dyn Source>();

    assert_send::<dyn Sink>();
    assert_sync::<dyn Sink>();

    assert_send::<dyn Framer>();
    assert_send::<dyn Decoder>();

    assert_send::<dyn LogSink>();
    assert_sync::<dyn LogSink>();

    assert_send::<dyn Importer>();
    assert_sync::<dyn Importer>();

    assert_send::<dyn Exporter>();
    assert_sync::<dyn Exporter>();

    assert_send::<dyn TimeseriesSink>();
    assert_sync::<dyn TimeseriesSink>();

    assert_send::<dyn TimeSource>();
    assert_sync::<dyn TimeSource>();
}
