use serde::Serialize;

#[repr(C)]
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
