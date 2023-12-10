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
    Download {
        #[arg(short)]
        output: PathBuf,
        torrent: PathBuf,
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

            let peers: Vec<SocketAddrV4> =
                torrent.get_peers(&info_hash).await.context("get peers")?;
            for peer in &peers {
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
            let peers: Vec<SocketAddrV4> =
                torrent.get_peers(&info_hash).await.context("get peers")?;
            for peer in &peers {
                println!("{}:{}", peer.ip(), peer.port());
            }
            let peers_len = peers.len();
            let range = rand::thread_rng().gen_range(0..peers_len);
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
            let piece_buf = torrent
                .download_piece(piece, &mut peer)
                .await
                .context("Download piece")?;

            tokio::fs::write(&output, piece_buf)
                .await
                .context("write out downloaded piece")?;
            println!("Piece {piece} downloaded to {}.", output.display());
        }
        Command::Download { output, torrent } => {
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
            let peers: Vec<SocketAddrV4> =
                torrent.get_peers(&info_hash).await.context("get peers")?;
            for peer in &peers {
                println!("{}:{}", peer.ip(), peer.port());
            }
            let peers_len = peers.len();
            let range = rand::thread_rng().gen_range(0..peers_len);
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

            //Receive Bitfield msg
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

            // Download file
            let res = torrent
                .download_file(&output, &mut peer)
                .await
                .context("Download file")?;

            println!("Downloaded test.torrent to {}.", output.display());
        }
    }

    Ok(())
}
