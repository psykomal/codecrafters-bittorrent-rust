use peers::Peers;
use serde::{Deserialize, Serialize, Serializer};

#[derive(Serialize, Clone, Debug)]
pub struct TrackerRequest {
    // info_hash: the info hash of the torrent
    // 20 bytes long, will need to be URL encoded
    // Note: this is NOT the hexadecimal representation, which is 40 bytes long
    // #[serde(serialize_with = "urlencode")]
    // pub info_hash: String,

    // peer_id: a unique identifier for your client
    // A string of length 20 that you get to pick. You can use something like 00112233445566778899.
    pub peer_id: String,

    // port: the port your client is listening on
    // You can set this to 6881, you will not have to support this functionality during this challenge.
    pub port: u16,

    // uploaded: the total amount uploaded so far
    // Since your client hasn't uploaded anything yet, you can set this to 0.
    pub uploaded: usize,

    // downloaded: the total amount downloaded so far
    // Since your client hasn't downloaded anything yet, you can set this to 0.
    pub downloaded: usize,

    // left: the number of bytes left to download
    // Since you client hasn't downloaded anything yet, this'll be the total length of the file (you've extracted this value from the torrent file in previous stages)
    pub left: usize,

    // compact: whether the peer list should use the compact representation
    // For the purposes of this challenge, set this to 1.
    // The compact representation is more commonly used in the wild, the non-compact representation is mostly supported for backward-compatibility.
    pub compact: u8,
}

#[derive(Deserialize, Clone, Debug)]
pub struct TrackerResponse {
    //     interval:
    // An integer, indicating how often your client should make a request to the tracker.
    // You can ignore this value for the purposes of this challenge.
    // peers.
    pub interval: usize,

    // A string, which contains list of peers that your client can connect to.
    // Each peer is represented using 6 bytes. The first 4 bytes are the peer's IP address and the last 2 bytes are the peer's port number.
    pub peers: Peers,
}

mod peers {
    use serde::{self, de::Visitor, Deserialize, Deserializer, Serialize, Serializer};
    use std::{
        fmt,
        net::{Ipv4Addr, SocketAddrV4},
    };

    #[derive(Debug, Clone)]
    pub struct Peers(pub Vec<SocketAddrV4>);

    struct PeersVisitor;

    impl<'de> Visitor<'de> for PeersVisitor {
        type Value = Peers;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("Each peer is represented using 6 bytes. The first 4 bytes are the peer's IP address and the last 2 bytes are the peer's port number.")
        }

        fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            if v.len() % 6 != 0 {
                return Err(serde::de::Error::invalid_length(v.len(), &self));
            }

            Ok(Peers(
                v.chunks_exact(6)
                    .into_iter()
                    .map(|x| {
                        SocketAddrV4::new(
                            Ipv4Addr::new(x[0], x[1], x[2], x[3]),
                            u16::from_be_bytes([x[4], x[5]]),
                        )
                    })
                    .collect(),
            ))
        }
    }

    impl<'de> Deserialize<'de> for Peers {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            deserializer.deserialize_bytes(PeersVisitor)
        }
    }
}

pub fn urlencode(t: &[u8; 20]) -> anyhow::Result<String> {
    let mut s = String::new();
    for b in t {
        s.push('%');
        s.push_str(&format!("{:02x}", b));
    }
    Ok(s)
}
