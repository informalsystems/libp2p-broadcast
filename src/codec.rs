use std::io;

use asynchronous_codec::{Decoder, Encoder};
use bytes::{Buf, BytesMut};

use crate::types::Message;

pub struct LengthPrefixedCodec {
    max_size: usize,
}

impl LengthPrefixedCodec {
    pub fn new(max_size: usize) -> Self {
        Self { max_size }
    }
}

impl Decoder for LengthPrefixedCodec {
    type Item = Message;
    type Error = io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let (msg_len, remaining) = match unsigned_varint::decode::usize(src) {
            Ok((len, remaining)) => (len, remaining),
            Err(unsigned_varint::decode::Error::Insufficient) => {
                // Not enough data to decode the length, wait for more
                return Ok(None);
            }
            Err(e) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Failed to decode length: {}", e),
                ))
            }
        };
        if msg_len > self.max_size {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "Received data size ({} bytes) exceeds maximum ({} bytes)",
                    msg_len, self.max_size
                ),
            ));
        }

        // Ensure we can read an entire message
        let varint_len = src.len() - remaining.len();
        if src.len() < varint_len + msg_len {
            return Ok(None);
        }

        // Safe to advance buffer now
        src.advance(varint_len);

        let msg = src.split_to(msg_len);

        match Message::from_bytes(&msg) {
            Ok(message) => Ok(Some(message)),
            Err(e) => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Failed to decode message: {}", e),
            )),
        }
    }
}

impl Encoder for LengthPrefixedCodec {
    type Item<'a> = Message;
    type Error = io::Error;

    fn encode(&mut self, item: Self::Item<'_>, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let msg_len = item.len();

        let mut varint_buf = unsigned_varint::encode::usize_buffer();
        let encoded_len = unsigned_varint::encode::usize(msg_len, &mut varint_buf);

        dst.extend_from_slice(&encoded_len);
        dst.extend_from_slice(&item.to_bytes());

        Ok(())
    }
}
