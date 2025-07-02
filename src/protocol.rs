use std::convert::Infallible;

use futures::future::{ready, Ready};
use libp2p::core::{InboundUpgrade, OutboundUpgrade, UpgradeInfo};
use libp2p::swarm::Stream;

const PROTOCOL_INFO: &str = "/ax/broadcast/1.0.0";

pub struct Protocol {}

impl UpgradeInfo for Protocol {
    type Info = &'static str;
    type InfoIter = std::iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        std::iter::once(PROTOCOL_INFO)
    }
}

impl InboundUpgrade<Stream> for Protocol {
    type Output = Stream;
    type Error = Infallible;
    type Future = Ready<Result<Self::Output, Self::Error>>;

    fn upgrade_inbound(self, socket: Stream, _: Self::Info) -> Self::Future {
        ready(Ok(socket))
    }
}

impl OutboundUpgrade<Stream> for Protocol {
    type Output = Stream;
    type Error = Infallible;
    type Future = Ready<Result<Self::Output, Self::Error>>;

    fn upgrade_outbound(self, socket: Stream, _: Self::Info) -> Self::Future {
        ready(Ok(socket))
    }
}
