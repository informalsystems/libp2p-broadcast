# libp2p-broadcast

The motivation for this protocol is to solve the substreams design issue of [`libp2p-scatter`](https://github.com/romac/libp2p-scatter). Indeed, the latter creates a new substream for each peer with each message sent. This approach is not efficient, as creating a new substream implies a new handshake for protocol negotiation, resulting in two round-trip times (RTTs) for each message sent. This additional RTT represents a significant overhead that reduces the protocol's throughput.

## Protocol design

The _libp2p handler_ (one per peer) utilizes two long-lived substreams: one for sending messages (outbound) and one for receiving messages (inbound). They are created only once, resulting in a single handshake for protocol negotiation.

Technically, the substreams are managed via a [`Framed`](https://crates.io/crates/asynchronous-codec) container, which provides a `Stream` and a `Sink` interface, allowing for the seamless asynchronous processing of messages. Moreover, the messages are encoded using a [length-prefixed codec](/src/codec.rs).

Note that the overall protocol interface remains unchanged compared to the original `libp2p-scatter` protocol, allowing it to be used as a drop-in replacement.
