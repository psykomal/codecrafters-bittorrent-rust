use anyhow::Context;
use bittorrent_starter_rust::{
    peer::{Handshake, Message, MessageFramer, MessageTag, PieceResponse, Request},
    torrent::*,
    tracker::{urlencode, TrackerRequest, TrackerResponse},
};
use clap::{Parser, Subcommand};
use futures_util::{SinkExt, StreamExt};
use hashes::Hashes;
use rand::Rng;
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

pub const BLOCK_MAX: u32 = 1 << 14;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Name of the person to greet
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
#[clap(rename_all = "snake_case")]
enum Command {
    Decode {
        value: String,
    },
    Info {
        torrent: PathBuf,
    },
    Peers {
        torrent: PathBuf,
    },
    Handshake {
        torrent: PathBuf,
        peer: String,
    },
    DownloadPiece {
        #[arg(short)]
        output: PathBuf,
        torrent: PathBuf,
        piece: usize,
    },
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

            let handshake = Handshake::new(info_hash, *b"00112233445566778899");

            peer.write_all(&bincode::serialize(&handshake).unwrap())
                .await?;

            let mut buf = [0; 100000];
            peer.read(&mut buf).await?;

            let handshake: Handshake = bincode::deserialize(&buf).unwrap();

            assert_eq!(handshake.length, 19);
            assert_eq!(&handshake.bittorrent, b"BitTorrent protocol");
            println!("Peer ID: {}", hex::encode(&handshake.peer_id));
        }
        Command::DownloadPiece {
            output,
            torrent,
            piece,
        } => {
            let dot_torrent = std::fs::read(torrent).context("read torrent file")?;
            let torrent: Torrent =
                serde_bencode::from_bytes(&dot_torrent).context("parse torrent file")?;
            // eprintln!("torrent: {:?}", torrent);

            let info_hash = torrent.info_hash();
            let length = if let Keys::SingleFile { length } = torrent.info.keys {
                length
            } else {
                0
            };

            // Tracker request for peers
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

            let response = reqwest::get(tracker_url).await?;
            let response = response.bytes().await?;
            let tracker_response: TrackerResponse =
                serde_bencode::from_bytes(&response).context("deserialize response")?;
            for peer in tracker_response.peers.0.iter() {
                println!("{}:{}", peer.ip(), peer.port());
            }
            let peers = tracker_response.peers.0;
            let range = rand::thread_rng().gen_range(0..peers.len());
            let peer = peers[range];

            // Handshake
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

            /// Download piece
            let mut peer = tokio_util::codec::Framed::new(peer, MessageFramer);

            // Receive Bitfield msg
            let msg = peer
                .next()
                .await
                .expect("peers always sends the first msg")
                .context("peer msg was invalid")?;
            // eprintln!("msg: {:?}", msg);
            assert_eq!(msg.tag, MessageTag::Bitfield);

            // Send interested msg
            peer.send(Message {
                tag: MessageTag::Interested,
                payload: vec![],
            })
            .await
            .context("send interested message")?;

            // recv unchoke
            let msg = peer
                .next()
                .await
                .expect("peer next msg")
                .context("peer msg was invalid")?;
            // eprintln!("msg: {:?}", msg);
            assert_eq!(msg.tag, MessageTag::Unchoke);

            // Download a piece
            let piece_length = torrent.info.piece_length as u32;
            let piece_hash = torrent.info.pieces.0[piece];
            let mut piece_buf: Vec<u8> = Vec::with_capacity(piece_length as usize);

            let mut start: u32 = 0;
            // eprintln!(
            //     "piece_length: {} num : {}",
            //     piece_length,
            //     f64::ceil(piece_length as f64 / BLOCK_MAX as f64)
            // );
            while start < piece_length {
                let l = if piece_length - start >= BLOCK_MAX {
                    BLOCK_MAX
                } else {
                    piece_length - start
                };
                let req = Request::new(piece as u32, start, l as u32);
                let req_bincode = bincode::serialize(&req).unwrap();

                // Send request msg
                peer.send(Message {
                    tag: MessageTag::Request,
                    payload: req_bincode,
                })
                .await
                .context("send request msg")?;

                // Recv piece msg
                let piece_msg = peer
                    .next()
                    .await
                    .expect("peer next msg")
                    .context("peer msg was invalid")?;
                // eprintln!("piece_msg: {:?}", piece_msg);
                assert_eq!(piece_msg.tag, MessageTag::Piece);

                let piece_response: PieceResponse = PieceResponse::from_bytes(&piece_msg.payload);
                eprintln!(
                    "p resp: {} {} {}",
                    u32::from_be_bytes(piece_response.index),
                    u32::from_be_bytes(piece_response.begin),
                    piece_response.block.len()
                );
                assert_eq!(u32::from_be_bytes(piece_response.index), piece as u32);
                assert_eq!(u32::from_be_bytes(piece_response.begin), start);

                // let mut block = piece_response.block;
                // block.extend(piece_buf);
                // piece_buf = block;
                piece_buf.extend(piece_response.block);

                start += BLOCK_MAX;
            }

            // piece_buf.reverse();

            assert_eq!(piece_buf.len(), piece_length as usize);

            // calc hash
            let mut hasher = Sha1::new();
            hasher.update(&piece_buf);
            let info_hash: [u8; 20] = hasher.finalize().into();
            assert_eq!(info_hash, piece_hash);

            tokio::fs::write(&output, piece_buf)
                .await
                .context("write out downloaded piece")?;
            println!("Piece {piece} downloaded to {}.", output.display());
        }
    }

    Ok(())
}
