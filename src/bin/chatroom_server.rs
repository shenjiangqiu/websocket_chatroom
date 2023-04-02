//! A chat server that broadcasts a message to all connections.
//!
//! This is a simple line-based server which accepts WebSocket connections,
//! reads lines from those connections, and broadcasts the lines to all other
//! connected clients.
//!
//! You can test this out by running:
//!
//!     cargo run --example server 127.0.0.1:12345
//!
//! And then in another window run:
//!
//!     cargo run --example client ws://127.0.0.1:12345/
//!
//! You can run the second command in multiple windows and then chat between the
//! two, seeing the messages from the other client as they're received. For all
//! connected clients they'll all join the same room and see everyone else's
//! messages.

use std::{
    collections::HashMap,
    env,
    io::Error as IoError,
    net::SocketAddr,
    sync::{Arc, Mutex},
};

use futures_channel::mpsc::{unbounded, UnboundedSender};
use futures_util::{future, pin_mut, stream::TryStreamExt, StreamExt};

use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;
use websocket_chatroom::{WebSocketClientToServerMessage, WebSocketServerToClientMessage};

type Tx = UnboundedSender<Message>;
type PeerMap = Arc<Mutex<HashMap<SocketAddr, (Tx, u32, String)>>>;

async fn handle_connection(
    peer_map: PeerMap,
    raw_stream: TcpStream,
    addr: SocketAddr,
    user_id: u32,
) -> eyre::Result<()> {
    println!("Incoming TCP connection from: {}", addr);

    let ws_stream = tokio_tungstenite::accept_async(raw_stream).await?;
    println!("WebSocket connection established: {}", addr);

    // Insert the write part of this peer to the peer map.
    let (tx, rx) = unbounded();

    let (outgoing, incoming) = ws_stream.split();
    let broadcast_incoming = incoming.try_for_each(|msg| {
        println!(
            "Received a message from {}: {}",
            addr,
            msg.to_text().unwrap()
        );
        let mut peers = peer_map.lock().unwrap();
        match msg {
            Message::Text(text) => {
                let message: WebSocketClientToServerMessage = serde_json::from_str(&text).unwrap();
                match message {
                    WebSocketClientToServerMessage::UserMessage(message_data) => {
                        // We want to broadcast the message to everyone except ourselves.
                        let broadcast_recipients = peers
                            .iter()
                            .filter(|(peer_addr, _)| peer_addr != &&addr)
                            .map(|(_, (ws_sink, ..))| ws_sink);
                        let message_server_to_client =
                            WebSocketServerToClientMessage::UserMessage(message_data);
                        let msg = Message::Text(
                            serde_json::to_string(&message_server_to_client).unwrap(),
                        );
                        for recp in broadcast_recipients {
                            recp.unbounded_send(msg.clone()).unwrap();
                        }
                    }
                    WebSocketClientToServerMessage::Connect(user_name) => {
                        peers.insert(addr, (tx.clone(), user_id, user_name.clone()));

                        let recipient = peers.get(&addr).unwrap();
                        let message_server_to_client =
                            WebSocketServerToClientMessage::Connected(user_id, user_name.clone());
                        let recipient_others = peers
                            .iter()
                            .filter(|(peer_addr, _)| peer_addr != &&addr)
                            .map(|(_, (ws_sink, ..))| ws_sink);
                        let others_message = WebSocketServerToClientMessage::NewUserAdded(
                            user_id,
                            user_name.clone(),
                        );
                        let all_usr_message = WebSocketServerToClientMessage::AllUsers(
                            peers
                                .iter()
                                .map(|(_, (_, user_id, user_name))| (*user_id, user_name.clone()))
                                .collect::<Vec<(u32, String)>>(),
                        );
                        let msg = Message::Text(
                            serde_json::to_string(&message_server_to_client).unwrap(),
                        );
                        let others_msg =
                            Message::Text(serde_json::to_string(&others_message).unwrap());
                        recipient.0.unbounded_send(msg).unwrap();
                        recipient
                            .0
                            .unbounded_send(Message::Text(
                                serde_json::to_string(&all_usr_message).unwrap(),
                            ))
                            .unwrap();
                        for recp in recipient_others {
                            recp.unbounded_send(others_msg.clone()).unwrap();
                        }
                    }
                }

                future::ok(())
            }
            _ => future::ok(()),
        }
    });

    let receive_from_others = rx.map(Ok).forward(outgoing);

    pin_mut!(broadcast_incoming, receive_from_others);
    future::select(broadcast_incoming, receive_from_others).await;

    println!("{} disconnected", &addr);
    let (_, id, name) = peer_map.lock().unwrap().remove(&addr).unwrap();
    let message_server_to_client = WebSocketServerToClientMessage::Disconnected(id, name);
    let msg = Message::Text(serde_json::to_string(&message_server_to_client).unwrap());
    for (_, (ws_sink, ..)) in peer_map.lock().unwrap().iter() {
        ws_sink.unbounded_send(msg.clone()).unwrap();
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), IoError> {
    let addr = env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:2233".to_string());

    let state = PeerMap::new(Mutex::new(HashMap::new()));

    // Create the event loop and TCP listener we'll accept connections on.
    let try_socket = TcpListener::bind(&addr).await;
    let listener = try_socket.expect("Failed to bind");
    println!("Listening on: {}", addr);

    // Let's spawn the handling of each connection in a separate task.
    let mut user_id = 0;
    while let Ok((stream, addr)) = listener.accept().await {
        tokio::spawn(handle_connection(state.clone(), stream, addr, user_id));
        user_id += 1;
    }

    Ok(())
}
