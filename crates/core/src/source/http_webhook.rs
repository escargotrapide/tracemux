//! HTTP-webhook [`Source`].
//!
//! v0.1 ships the trait surface only — the actual HTTP listener
//! (axum) is feature-gated and delivered via the `add-source` skill.
//! Calling [`HttpWebhookSource::open`] without the `http-webhook`
//! feature surfaces `E-1101` with a clear message.

use std::collections::BTreeMap;

use async_trait::async_trait;

use super::{ChannelMeta, ControlEvt, Frame, Source};
use crate::{ErrorId, Result, WanloggerError};

/// HTTP-webhook source (stub).
#[derive(Debug)]
pub struct HttpWebhookSource {
    bind: String,
    path: String,
}

impl HttpWebhookSource {
    /// Construct.
    #[must_use]
    pub fn new(bind: impl Into<String>, path: impl Into<String>) -> Self {
        Self {
            bind: bind.into(),
            path: path.into(),
        }
    }
}

#[async_trait]
impl Source for HttpWebhookSource {
    async fn open(&mut self) -> Result<()> {
        Err(WanloggerError::new(
            ErrorId::E1101SourceOpen,
            format!(
                "http-webhook source disabled in this build: enable the \
                 `http-webhook` feature to listen on {} {}",
                self.bind, self.path
            ),
        ))
    }

    async fn recv(&mut self) -> Result<Option<Frame>> {
        Err(WanloggerError::new(
            ErrorId::E1102SourceClosed,
            "http-webhook source not open",
        ))
    }

    async fn recv_ctl(&mut self) -> Result<Option<ControlEvt>> {
        Ok(None)
    }

    fn metadata(&self) -> ChannelMeta {
        ChannelMeta {
            kind: "http-webhook".into(),
            iface: format!("{}{}", self.bind, self.path),
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
        let mut s = HttpWebhookSource::new("127.0.0.1:0", "/webhook");
        let err = s.open().await.unwrap_err();
        assert_eq!(err.id, ErrorId::E1101SourceOpen);
    }
}
