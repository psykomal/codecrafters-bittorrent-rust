use serde_json;
use std::{collections::BTreeMap, env};

// Available if you need it!
// use serde_bencode

#[allow(dead_code)]
fn decode_bencoded_value(encoded_value: &str) -> (serde_json::Value, &str) {
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

// Usage: your_bittorrent.sh decode "<encoded_value>"
fn main() {
    let args: Vec<String> = env::args().collect();
    let command = &args[1];

    if command == "decode" {
        // You can use print statements as follows for debugging, they'll be visible when running tests.
        // println!("Logs from your program will appear here!");

        // Uncomment this block to pass the first stage
        let encoded_value = &args[2];
        let decoded_value = decode_bencoded_value(encoded_value);
        println!("{}", decoded_value.0.to_string());
    } else {
        println!("unknown command: {}", args[1])
    }
}
