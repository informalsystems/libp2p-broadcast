use std::{
    collections::VecDeque,
    pin::Pin,
    task::{Context, Poll},
};

use asynchronous_codec::Framed;
use futures::prelude::*;
use libp2p::swarm::{
    handler::{ConnectionEvent, DialUpgradeError, FullyNegotiatedInbound, FullyNegotiatedOutbound},
    ConnectionHandler, ConnectionHandlerEvent, Stream, SubstreamProtocol,
};

use crate::{codec::LengthPrefixedCodec, config::Config, protocol::Protocol, types::Message};

#[derive(Debug)]
pub enum HandlerEvent {
    /// We received a `Message` from a remote.
    Rx(Message),
    /// We successfully sent a `Message`.
    Tx,
}

enum InboundSubstreamState {
    /// Waiting for an inbound message. The idle state for an inbound substream.
    WaitingInput(Framed<Stream, LengthPrefixedCodec>),
    /// The substream is being closed.
    Closing(Framed<Stream, LengthPrefixedCodec>),
    /// An error occurred during processing.
    Poisoned,
}

enum OutboundSubstreamState {
    /// Waiting for an outbound message to be sent. The idle state for an outbound
    /// substream.
    WaitingOutput(Framed<Stream, LengthPrefixedCodec>),
    /// Waiting to send an outbound message.
    PendingSend(Framed<Stream, LengthPrefixedCodec>, Message),
    /// Waiting to flush the substream.
    PendingFlush(Framed<Stream, LengthPrefixedCodec>),
    /// An error occurred during processing.
    Poisoned,
}

pub struct Handler {
    config: Config,

    /// The single long-lived inbound substream.
    inbound_substream: Option<InboundSubstreamState>,
    /// The single long-lived outbound substream.
    outbound_substream: Option<OutboundSubstreamState>,
    /// Flag indicating that an outbound substream is being established to prevent
    /// concurrent establishment attempts.
    establishing_outbound_substream: bool,

    /// Queue of messages that are pending to be sent.
    pending_messages: VecDeque<Message>,
}

impl Handler {
    pub(super) fn new(config: Config) -> Self {
        Self {
            config,
            inbound_substream: None,
            outbound_substream: None,
            establishing_outbound_substream: false,
            pending_messages: VecDeque::new(),
        }
    }

    fn on_fully_negotiated_inbound(
        &mut self,
        FullyNegotiatedInbound {
            protocol: stream,
            info: (),
        }: FullyNegotiatedInbound<<Self as ConnectionHandler>::InboundProtocol>,
    ) {
        self.inbound_substream = Some(InboundSubstreamState::WaitingInput(Framed::new(
            stream,
            LengthPrefixedCodec::new(self.config.max_buf_size),
        )))
    }

    fn on_fully_negotiated_outbound(
        &mut self,
        FullyNegotiatedOutbound {
            protocol: stream,
            info: (),
        }: FullyNegotiatedOutbound<<Self as ConnectionHandler>::OutboundProtocol>,
    ) {
        assert!(
            self.outbound_substream.is_none(),
            "Established an outbound substream with one already available"
        );

        self.outbound_substream = Some(OutboundSubstreamState::WaitingOutput(Framed::new(
            stream,
            LengthPrefixedCodec::new(self.config.max_buf_size),
        )));
    }

    fn on_dial_upgrade_error(
        &mut self,
        DialUpgradeError { error, info: () }: DialUpgradeError<
            (),
            <Self as ConnectionHandler>::OutboundProtocol,
        >,
    ) {
        tracing::warn!(
            "{}",
            format!(
                "Dial upgrade error, dropping {} messages: {:?}",
                self.pending_messages.drain(..).count(),
                error
            )
        );
    }
}

impl ConnectionHandler for Handler {
    type FromBehaviour = Message;
    type ToBehaviour = HandlerEvent;
    type InboundProtocol = Protocol;
    type OutboundProtocol = Protocol;
    type InboundOpenInfo = ();
    type OutboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol> {
        SubstreamProtocol::new(Protocol {}, ())
    }

    fn on_behaviour_event(&mut self, msg: Self::FromBehaviour) {
        self.pending_messages.push_back(msg);
    }

    #[tracing::instrument(level = "trace", name = "ConnectionHandler::poll", skip(self, cx))]
    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<ConnectionHandlerEvent<Self::OutboundProtocol, (), Self::ToBehaviour>> {
        // Determine if we need to create an outbound substream
        if !self.pending_messages.is_empty()
            && self.outbound_substream.is_none()
            && !self.establishing_outbound_substream
        {
            self.establishing_outbound_substream = true;
            return Poll::Ready(ConnectionHandlerEvent::OutboundSubstreamRequest {
                protocol: SubstreamProtocol::new(Protocol {}, ()),
            });
        }

        // Handle inbound substream
        loop {
            match self
                .inbound_substream
                .replace(InboundSubstreamState::Poisoned)
            {
                Some(InboundSubstreamState::WaitingInput(mut substream)) => {
                    match substream.poll_next_unpin(cx) {
                        Poll::Ready(Some(Ok(message))) => {
                            self.inbound_substream =
                                Some(InboundSubstreamState::WaitingInput(substream));
                            return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                                HandlerEvent::Rx(message),
                            ));
                        }
                        Poll::Ready(Some(Err(e))) => {
                            // Close this side of the substream. If the peer is still around,
                            // they will re-establish their outbound substream, i.e., our inbound substream.
                            tracing::debug!("Inbound substream error: {e}");
                            self.inbound_substream =
                                Some(InboundSubstreamState::Closing(substream));
                            break;
                        }
                        Poll::Ready(None) => {
                            tracing::debug!("Inbound substream closed by remote");
                            self.inbound_substream =
                                Some(InboundSubstreamState::Closing(substream));
                        }
                        Poll::Pending => {
                            self.inbound_substream =
                                Some(InboundSubstreamState::WaitingInput(substream));
                            break;
                        }
                    }
                }
                Some(InboundSubstreamState::Closing(mut substream)) => {
                    match Sink::poll_close(Pin::new(&mut substream), cx) {
                        Poll::Ready(res) => {
                            if let Err(e) = res {
                                // Don't close the connection but just drop the inbound substream.
                                // In case the remote has more to send, the will open up a new
                                // substream.
                                tracing::debug!("Inbound substream error while closing: {e}");
                            }
                            self.inbound_substream = None;
                            break;
                        }
                        Poll::Pending => {
                            self.inbound_substream =
                                Some(InboundSubstreamState::Closing(substream));
                            break;
                        }
                    }
                }
                None => {
                    self.inbound_substream = None;
                    break;
                }
                Some(InboundSubstreamState::Poisoned) => {
                    unreachable!("Error occurred during inbound substream processing")
                }
            }
        }

        // Process outbound substream
        loop {
            match self
                .outbound_substream
                .replace(OutboundSubstreamState::Poisoned)
            {
                Some(OutboundSubstreamState::WaitingOutput(substream)) => {
                    if let Some(message) = self.pending_messages.pop_front() {
                        self.outbound_substream =
                            Some(OutboundSubstreamState::PendingSend(substream, message));
                        continue;
                    }

                    self.outbound_substream =
                        Some(OutboundSubstreamState::WaitingOutput(substream));
                    break;
                }
                Some(OutboundSubstreamState::PendingSend(mut substream, message)) => {
                    match Sink::poll_ready(Pin::new(&mut substream), cx) {
                        Poll::Ready(Ok(())) => {
                            match Sink::start_send(Pin::new(&mut substream), message) {
                                Ok(()) => {
                                    self.outbound_substream =
                                        Some(OutboundSubstreamState::PendingFlush(substream));
                                }
                                Err(e) => {
                                    tracing::debug!(
                                        "Failed to send message on outbound substream: {e}"
                                    );
                                    self.outbound_substream = None;
                                    break;
                                }
                            }
                        }
                        Poll::Ready(Err(e)) => {
                            tracing::debug!("Failed to send message on outbound substream: {e}");
                            self.outbound_substream = None;
                            break;
                        }
                        Poll::Pending => {
                            self.outbound_substream =
                                Some(OutboundSubstreamState::PendingSend(substream, message));
                            break;
                        }
                    }
                }
                Some(OutboundSubstreamState::PendingFlush(mut substream)) => {
                    match Sink::poll_flush(Pin::new(&mut substream), cx) {
                        Poll::Ready(Ok(())) => {
                            self.outbound_substream =
                                Some(OutboundSubstreamState::WaitingOutput(substream));
                        }
                        Poll::Ready(Err(e)) => {
                            tracing::debug!("Failed to flush outbound substream: {e}");
                            self.outbound_substream = None;
                            break;
                        }
                        Poll::Pending => {
                            self.outbound_substream =
                                Some(OutboundSubstreamState::PendingFlush(substream));
                            break;
                        }
                    }
                }
                None => {
                    self.outbound_substream = None;
                    break;
                }
                Some(OutboundSubstreamState::Poisoned) => {
                    unreachable!("Error occurred during outbound substream processing")
                }
            }
        }

        Poll::Pending
    }

    fn on_connection_event(
        &mut self,
        event: ConnectionEvent<Self::InboundProtocol, Self::OutboundProtocol>,
    ) {
        match event {
            ConnectionEvent::FullyNegotiatedInbound(fully_negotiated_inbound) => {
                self.on_fully_negotiated_inbound(fully_negotiated_inbound)
            }
            ConnectionEvent::FullyNegotiatedOutbound(fully_negotiated_outbound) => {
                self.on_fully_negotiated_outbound(fully_negotiated_outbound)
            }
            ConnectionEvent::DialUpgradeError(dial_upgrade_error) => {
                self.on_dial_upgrade_error(dial_upgrade_error)
            }
            _ => {}
        }
    }
}
