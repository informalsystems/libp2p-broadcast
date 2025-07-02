use std::{
    fmt,
    io::{Error, ErrorKind, Result},
};

use bytes::Bytes;
use prometheus_client::encoding::{EncodeLabelSet, LabelSetEncoder};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Topic {
    len: u8,
    bytes: [u8; 64],
}

impl Topic {
    pub const MAX_TOPIC_LENGTH: usize = 64;

    pub fn new(topic: &[u8]) -> Self {
        let mut bytes = [0u8; 64];
        bytes[..topic.len()].copy_from_slice(topic);
        Self {
            len: topic.len() as _,
            bytes,
        }
    }
}

impl EncodeLabelSet for Topic {
    fn encode(&self, mut encoder: LabelSetEncoder) -> fmt::Result {
        use prometheus_client::encoding::{EncodeLabelKey, EncodeLabelValue};

        let mut label_encoder = encoder.encode_label();
        let mut key_encoder = label_encoder.encode_label_key()?;
        EncodeLabelKey::encode(&"topic", &mut key_encoder)?;
        let mut value_encoder = key_encoder.encode_label_value()?;
        let value = String::from_utf8_lossy(self.as_ref());
        EncodeLabelValue::encode(&value, &mut value_encoder)?;
        value_encoder.finish()
    }
}

impl std::ops::Deref for Topic {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.as_ref()
    }
}

impl AsRef<[u8]> for Topic {
    fn as_ref(&self) -> &[u8] {
        &self.bytes[..(self.len as usize)]
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Message {
    Subscribe(Topic),
    Broadcast(Topic, Bytes),
    Unsubscribe(Topic),
}

impl Message {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.is_empty() {
            return Err(Error::new(ErrorKind::InvalidData, "empty message"));
        }
        let topic_len = (bytes[0] >> 2) as usize;
        if bytes.len() < topic_len + 1 {
            return Err(Error::new(
                ErrorKind::InvalidData,
                "topic length out of range",
            ));
        }
        let msg_len = bytes.len() - topic_len - 1;
        let topic = Topic::new(&bytes[1..topic_len + 1]);
        Ok(match bytes[0] & 0b11 {
            0b00 => Message::Subscribe(topic),
            0b10 => Message::Unsubscribe(topic),
            0b01 => {
                let mut msg = Vec::with_capacity(msg_len);
                msg.extend_from_slice(&bytes[(topic_len + 1)..]);
                Message::Broadcast(topic, msg.into())
            }
            _ => return Err(Error::new(ErrorKind::InvalidData, "invalid header")),
        })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        match self {
            Message::Subscribe(topic) => {
                let mut buf = Vec::with_capacity(topic.len() + 1);
                buf.push((topic.len() as u8) << 2);
                buf.extend_from_slice(topic);
                buf
            }
            Message::Unsubscribe(topic) => {
                let mut buf = Vec::with_capacity(topic.len() + 1);
                buf.push((topic.len() as u8) << 2 | 0b10);
                buf.extend_from_slice(topic);
                buf
            }
            Message::Broadcast(topic, msg) => {
                let mut buf = Vec::with_capacity(topic.len() + msg.len() + 1);
                buf.push((topic.len() as u8) << 2 | 0b01);
                buf.extend_from_slice(topic);
                buf.extend_from_slice(msg);
                buf
            }
        }
    }

    pub fn len(&self) -> usize {
        match self {
            Message::Subscribe(topic) => 1 + topic.len(),
            Message::Unsubscribe(topic) => 1 + topic.len(),
            Message::Broadcast(topic, msg) => 1 + topic.len() + msg.len(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip() {
        let topic = Topic::new(b"topic");
        let msgs = [
            Message::Broadcast(Topic::new(b""), Bytes::from_static(b"")),
            Message::Subscribe(topic),
            Message::Unsubscribe(topic),
            Message::Broadcast(topic, Bytes::from_static(b"content")),
        ];
        for msg in &msgs {
            let msg2 = Message::from_bytes(&msg.to_bytes()).unwrap();
            assert_eq!(msg, &msg2);
        }
    }

    #[test]
    #[should_panic]
    fn test_invalid_message() {
        let out_of_range = [0b0000_0100];
        Message::from_bytes(&out_of_range).unwrap();
    }
}
