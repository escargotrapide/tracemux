//! TEMPLATE — replace `Stub` with your source name and finish the
//! TODOs marked below. See `.github/skills/add-source/SKILL.md`.

use async_trait::async_trait;

use tracemux_core::source::{ChannelMeta, ControlEvt, Frame, Source};
use tracemux_core::Result;

/// TODO: rename me.
#[derive(Debug, Default)]
pub struct StubSource;

#[async_trait]
impl Source for StubSource {
    async fn open(&mut self) -> Result<()> {
        // TODO: open the underlying transport.
        Ok(())
    }
    async fn recv(&mut self) -> Result<Option<Frame>> {
        // TODO: produce frames; return None on EOF.
        Ok(None)
    }
    async fn recv_ctl(&mut self) -> Result<Option<ControlEvt>> {
        Ok(None)
    }
    fn metadata(&self) -> ChannelMeta {
        ChannelMeta {
            kind: "stub".into(),
            iface: "n/a".into(),
            tags: Default::default(),
        }
    }
    async fn close(&mut self) -> Result<()> {
        Ok(())
    }
}
