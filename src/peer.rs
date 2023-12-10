use bytes::BufMut;
use bytes::{Buf, BytesMut};
use serde::{self, de::Visitor, Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use tokio_util::codec::Decoder;
use tokio_util::codec::Encoder;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Handshake {
    //     length of the protocol string (BitTorrent protocol) which is 19 (1 byte)
    pub length: u8,
    // the string BitTorrent protocol (19 bytes)
    pub bittorrent: [u8; 19],
    // eight reserved bytes, which are all set to zero (8 bytes)
    pub reserved: [u8; 8],
    // sha1 infohash (20 bytes) (NOT the hexadecimal representation, which is 40 bytes long)
    pub info_hash: [u8; 20],
    // peer id (20 bytes) (you can use 00112233445566778899 for this challenge)
    pub peer_id: [u8; 20],
}

impl Handshake {
    pub fn new(info_hash: [u8; 20], peer_id: [u8; 20]) -> Self {
        Self {
            length: 19,
            bittorrent: *b"BitTorrent protocol",
            reserved: [0; 8],
            info_hash,
            peer_id,
        }
    }
}

/// Peer message type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MessageTag {
    Choke = 0,
    Unchoke = 1,
    Interested = 2,
    NotInterested = 3,
    Have = 4,
    Bitfield = 5,
    Request = 6,
    Piece = 7,
    Cancel = 8,
}

impl MessageTag {
    fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(MessageTag::Choke),
            1 => Some(MessageTag::Unchoke),
            2 => Some(MessageTag::Interested),
            3 => Some(MessageTag::NotInterested),
            4 => Some(MessageTag::Have),
            5 => Some(MessageTag::Bitfield),
            6 => Some(MessageTag::Request),
            7 => Some(MessageTag::Piece),
            8 => Some(MessageTag::Cancel),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Message {
    pub tag: MessageTag,
    pub payload: Vec<u8>,
}

#[derive(Serialize)]
pub struct Request {
    index: [u8; 4],
    begin: [u8; 4],
    length: [u8; 4],
}

impl Request {
    pub fn new(index: u32, begin: u32, length: u32) -> Self {
        Request {
            index: index.to_be_bytes(),
            begin: begin.to_be_bytes(),
            length: length.to_be_bytes(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PieceResponse {
    pub index: [u8; 4],
    pub begin: [u8; 4],
    pub block: Vec<u8>,
}

impl PieceResponse {
    pub fn from_bytes(b: &[u8]) -> Self {
        PieceResponse {
            index: [b[0], b[1], b[2], b[3]],
            begin: [b[4], b[5], b[6], b[7]],
            block: b[8..].to_vec(),
        }
    }
}

pub struct MessageFramer;

const MAX: usize = 1 << 16;

impl Decoder for MessageFramer {
    type Item = Message;
    type Error = std::io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if src.len() < 4 {
            // Not enough data to read length marker + tag.
            return Ok(None);
        }

        // Read length marker.
        let mut length_bytes = [0u8; 4];
        length_bytes.copy_from_slice(&src[..4]);
        let length = u32::from_be_bytes(length_bytes) as usize;

        if length == 0 {
            // hearbeat msg
            // discard it
            src.advance(4);
            // try again in case buffer has more msgs
            return self.decode(src);
        }
        if src.len() < 5 {
            // Not enough data to read length marker + tag.
            return Ok(None);
        }

        // Check that the length is not too large to avoid a denial of
        // service attack where the server runs out of memory.
        if length > MAX {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Frame of length {} is too large.", length),
            ));
        }

        if src.len() < 4 + length {
            // The full string has not yet arrived.
            //
            // We reserve more space in the buffer. This is not strictly
            // necessary, but is a good idea performance-wise.
            src.reserve(4 + length - src.len());

            // We inform the Framed that we need more bytes to form the next
            // frame.
            return Ok(None);
        }

        // Use advance to modify src such that it no longer contains
        // this frame.
        let tag = src[4];
        let data = src[5..4 + length].to_vec();
        src.advance(4 + length);

        Ok(Some(Message {
            tag: MessageTag::from_u8(tag).expect("valid messagetag"),
            payload: data,
        }))
    }
}

impl Encoder<Message> for MessageFramer {
    type Error = std::io::Error;

    fn encode(&mut self, item: Message, dst: &mut BytesMut) -> Result<(), Self::Error> {
        // Don't send a msg if it is longer than the other end will
        // accept.
        if item.payload.len() + 1 > MAX {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Frame of length {} is too large.", item.payload.len() + 1),
            ));
        }

        let len_slice = u32::to_be_bytes(item.payload.len() as u32 + 1);

        // Reserve space in the buffer.
        dst.reserve(4 + 1 + item.payload.len());

        // Write the length, tag and payload to the buffer.
        dst.extend_from_slice(&len_slice);
        dst.put_u8(item.tag as u8);
        dst.extend_from_slice(&item.payload);
        Ok(())
    }
}
