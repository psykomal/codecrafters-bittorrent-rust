use anyhow::Context;
use bittorrent_starter_rust::{
    peer::Handshake,
    torrent::*,
    tracker::{urlencode, TrackerRequest, TrackerResponse},
};
use clap::{Parser, Subcommand};
use hashes::Hashes;
use reqwest;
use serde::{self, Deserialize, Serialize};
use serde_json;
use sha1::{Digest, Sha1};
use std::{
    fmt::format,
    net::SocketAddrV4,
    path::{Path, PathBuf},
    str::FromStr,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Name of the person to greet
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    Decode { value: String },
    Info { torrent: PathBuf },
    Peers { torrent: PathBuf },
    Handshake { torrent: PathBuf, peer: String },
}

#[tokio::main]
// Usage: your_bittorrent.sh decode "<encoded_value>"
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    match args.command {
        Command::Decode { value } => {
            let v = decode_bencoded_value(&value);
            println!("{}", v.0);
        }
        Command::Info { torrent } => {
            let dot_torrent = std::fs::read(torrent).context("read torrent file")?;
            let torrent: Torrent =
                serde_bencode::from_bytes(&dot_torrent).context("parse torrent file")?;

            let info_hash = torrent.info_hash();

            println!("Tracker URL: {}", torrent.announce);
            let length = if let Keys::SingleFile { length } = torrent.info.keys {
                length
            } else {
                0
            };
            println!("Length: {}", length);
            println!("Info Hash: {}", hex::encode(info_hash));
            println!("Piece Length: {}", torrent.info.piece_length);
            println!("Piece Hashes:");
            for piece in torrent.info.pieces.0 {
                println!("{}", hex::encode(piece));
            }
        }
        Command::Peers { torrent } => {
            let dot_torrent = std::fs::read(torrent).context("read torrent file")?;
            let torrent: Torrent =
                serde_bencode::from_bytes(&dot_torrent).context("parse torrent file")?;

            let info_hash = torrent.info_hash();
            let length = if let Keys::SingleFile { length } = torrent.info.keys {
                length
            } else {
                0
            };

            let request = TrackerRequest {
                peer_id: String::from("00112233445566778899"),
                port: 6881,
                uploaded: 0,
                downloaded: 0,
                left: length,
                compact: 1,
            };

            let url_params =
                serde_urlencoded::to_string(&request).context("Request to URL params")?;
            let tracker_url = format!(
                "{}?{}&info_hash={}",
                torrent.announce,
                url_params,
                urlencode(&info_hash).expect("encode info hash")
            );

            // println!("Tracker URL: {}", tracker_url);
            let response = reqwest::get(tracker_url).await?;
            let response = response.bytes().await?;
            // println!("Response: {:?}", &response);
            let tracker_response: TrackerResponse =
                serde_bencode::from_bytes(&response).context("deserialize response")?;
            // println!("Tracker Response: {:?}", tracker_response);
            for peer in tracker_response.peers.0.iter() {
                println!("{}:{}", peer.ip(), peer.port());
            }
        }
        Command::Handshake { torrent, peer } => {
            let dot_torrent = std::fs::read(torrent).context("read torrent file")?;
            let torrent: Torrent =
                serde_bencode::from_bytes(&dot_torrent).context("parse torrent file")?;

            let info_hash = torrent.info_hash();
            let peer = SocketAddrV4::from_str(&peer).context("parse peer address")?;
            let mut peer = tokio::net::TcpStream::connect(peer)
                .await
                .context("connect to peer")?;

            let mut handshake = Handshake::new(info_hash, *b"00112233445566778899");

            peer.write_all(&bincode::serialize(&handshake).unwrap())
                .await?;

            let mut buf = [0; 68];
            peer.read_exact(&mut buf).await?;

            let handshake: Handshake = bincode::deserialize(&buf).unwrap();

            assert_eq!(handshake.length, 19);
            assert_eq!(&handshake.bittorrent, b"BitTorrent protocol");
            println!("Peer ID: {}", hex::encode(&handshake.peer_id));
        }
    }

    Ok(())
}
