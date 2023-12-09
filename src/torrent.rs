use anyhow::Context;
use clap::{Parser, Subcommand};
use hashes::Hashes;
use serde::{self, Deserialize, Serialize};
use serde_json;
use sha1::{Digest, Sha1};
use std::path::PathBuf;

/// Metainfo files (also known as .torrent files) are bencoded dictionaries with the following keys:
#[derive(Debug, Clone, Deserialize)]
pub struct Torrent {
    // The URL of the tracker.
    pub announce: String,
    // This maps to a dictionary, with keys described below.
    pub info: Info,
}

impl Torrent {
    pub fn info_hash(&self) -> [u8; 20] {
        let info_bencode = serde_bencode::to_bytes(&self.info)
            .context("serialize info")
            .unwrap();

        let mut hasher = Sha1::new();
        hasher.update(info_bencode);
        let info_hash = hasher.finalize();
        info_hash.into()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Info {
    // The name key maps to a UTF-8 encoded string which is the suggested name to save the file (or directory) as.
    // In the single file case, the name key is the name of a file, in the muliple file case, it's the name of a directory.
    pub name: String,

    // piece length maps to the number of bytes in each piece the file is split into. For the purposes of transfer,
    // files are split into fixed-size pieces which are all the same length except for possibly the last one which may be truncated.
    // piece length is almost always a power of two, most commonly 2^18 = 256 K (BitTorrent prior to version 3.2 uses 2^20 = 1 M as default).
    #[serde(rename = "piece length")]
    pub piece_length: usize,

    // pieces maps to a string whose length is a multiple of 20. It is to be subdivided into strings of length 20,
    // each of which is the SHA1 hash of the piece at the corresponding index.
    pub pieces: Hashes,

    // There is also a key length or a key files, but not both or neither.
    #[serde(flatten)]
    pub keys: Keys,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Keys {
    // If length is present then the download represents a single file,
    // In the single file case, length maps to the length of the file in bytes.
    SingleFile { length: usize },

    // otherwise it represents a set of files which go in a directory structure.
    // For the purposes of the other keys, the multi-file case is treated as only having a single file by
    // concatenating the files in the order they appear in the files list. The files list is the value files maps to,
    // and is a list of dictionaries containing the following keys:
    // length - The length of the file, in bytes.
    // path - A list of UTF-8 encoded strings corresponding to subdirectory names, the last of which is the actual file name (a zero length list is an error case).
    MultiFile { files: Vec<File> },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct File {
    pub length: usize,
    pub path: Vec<String>,
}

pub mod hashes {
    use serde::{self, de::Visitor, Deserialize, Deserializer, Serialize, Serializer};
    use std::fmt;

    #[derive(Debug, Clone)]
    pub struct Hashes(pub Vec<[u8; 20]>);
    struct HashesVisitor;

    impl<'de> Visitor<'de> for HashesVisitor {
        type Value = Hashes;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a byte string whose length multiple of 20")
        }

        fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            if v.len() % 20 != 0 {
                return Err(serde::de::Error::invalid_length(v.len(), &self));
            }

            Ok(Hashes(
                v.chunks_exact(20)
                    .map(|slice_20| slice_20.try_into().expect("guranteed to be length 20"))
                    .collect(),
            ))
        }
    }

    impl<'de> Deserialize<'de> for Hashes {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            deserializer.deserialize_bytes(HashesVisitor)
        }
    }

    impl Serialize for Hashes {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            let v = self.0.concat();
            serializer.serialize_bytes(&v)
        }
    }
}

pub fn decode_bencoded_value(encoded_value: &str) -> (serde_json::Value, &str) {
    match encoded_value.chars().next() {
        Some('i') => {
            let (_, rest) = encoded_value.split_once("i").unwrap();
            let (n, rest) = rest.split_once("e").unwrap();
            let n = n.parse::<i64>().unwrap();
            return (n.into(), rest);
        }
        Some('l') => {
            let (_, mut rest) = encoded_value.split_once("l").unwrap();
            let mut list = Vec::new();
            while !rest.is_empty() && !rest.starts_with("e") {
                let (value, rem) = decode_bencoded_value(rest);
                list.push(value);
                rest = rem;
            }
            return (list.into(), &rest[1..]);
        }
        Some('d') => {
            let (_, mut rest) = encoded_value.split_once("d").unwrap();
            let mut dict = serde_json::Map::new();
            while !rest.is_empty() && !rest.starts_with("e") {
                let (k, rem) = decode_bencoded_value(rest);
                let k = match k {
                    serde_json::Value::String(s) => s,
                    _ => panic!("Unexpected key type"),
                };

                let (v, rem) = decode_bencoded_value(rem);
                dict.insert(k, v);
                rest = rem;
            }
            return (dict.into(), &rest[1..]);
        }
        Some('0'..='9') => {
            let (n, rest) = encoded_value.split_once(":").unwrap();
            let n = n.parse::<usize>().unwrap();
            return (serde_json::Value::String(rest[..n].to_string()), &rest[n..]);
        }
        _ => {
            panic!("Unexpected end of encoded value")
        }
    }
}
