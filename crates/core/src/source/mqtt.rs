//! MQTT subscriber [`Source`].
//!
//! v0.1 ships the trait surface only — wiring to a real MQTT
//! client (e.g. `rumqttc`) is feature-gated and delivered via the
//! `add-source` skill. Calling [`MqttSource::open`] without the
//! `mqtt` feature surfaces `E-1101` with a clear message.

use std::collections::BTreeMap;

use async_trait::async_trait;

use super::{ChannelMeta, ControlEvt, Frame, Source};
use crate::{ErrorId, Result, WanloggerError};

/// MQTT subscriber source (stub until the `mqtt` feature is added).
#[derive(Debug)]
pub struct MqttSource {
    broker: String,
    topic: String,
}

impl MqttSource {
    /// Construct.
    #[must_use]
    pub fn new(broker: impl Into<String>, topic: impl Into<String>) -> Self {
        Self {
            broker: broker.into(),
            topic: topic.into(),
        }
    }
}

#[async_trait]
impl Source for MqttSource {
    async fn open(&mut self) -> Result<()> {
        Err(WanloggerError::new(
            ErrorId::E1101SourceOpen,
            format!(
                "mqtt source disabled in this build: enable the `mqtt` feature \
                 to subscribe to {} on {}",
                self.topic, self.broker
            ),
        ))
    }

    async fn recv(&mut self) -> Result<Option<Frame>> {
        Err(WanloggerError::new(
            ErrorId::E1102SourceClosed,
            "mqtt source not open",
        ))
    }

    async fn recv_ctl(&mut self) -> Result<Option<ControlEvt>> {
        Ok(None)
    }

    fn metadata(&self) -> ChannelMeta {
        ChannelMeta {
            kind: "mqtt".into(),
            iface: format!("{}:{}", self.broker, self.topic),
            tags: BTreeMap::new(),
        }
    }

    async fn close(&mut self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn open_returns_e1101_until_feature_added() {
        let mut s = MqttSource::new("tcp://localhost:1883", "sensors/+");
        let err = s.open().await.unwrap_err();
        assert_eq!(err.id, ErrorId::E1101SourceOpen);
    }
}
