use iced::{
    futures::{SinkExt, StreamExt},
    subscription, Subscription,
};
use serde::{Deserialize, Serialize};
use tokio::{
    net::TcpStream,
    sync::mpsc::{Receiver, Sender},
};
use tokio_tungstenite::{tungstenite::Message, MaybeTlsStream, WebSocketStream};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MessageData {
    pub id: u32,
    pub name: String,
    pub data: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum WebSocketMessage {
    UserMessage(MessageData),
}

pub fn connect(server_socket: &str) -> Subscription<Event> {
    struct Connect;
    let server_socket = server_socket.to_string();
    subscription::unfold(
        std::any::TypeId::of::<Connect>(),
        State::Disconnected,
        move |state| {
            let server_socket = server_socket.clone();
            async move {
                match state {
                    State::Disconnected => {
                        match tokio_tungstenite::connect_async(server_socket).await {
                            Ok((websocket, _)) => {
                                let (sender, receiver) = tokio::sync::mpsc::channel(10);

                                (
                                    Some(Event::Connected(Connection(sender))),
                                    State::Connected(websocket, receiver),
                                )
                            }
                            Err(_) => {
                                // Wait for 1 second before retrying
                                println!("Connection failed... Retrying...");
                                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

                                (Some(Event::Disconnected), State::Disconnected)
                            }
                        }
                    }
                    State::Connected(mut websocket, mut input) => {
                        let mut fused_websocket = websocket.by_ref().fuse();
                        let on_receive_remote =
                            |received,
                             websocket: WebSocketStream<MaybeTlsStream<TcpStream>>,
                             input: Receiver<WebSocketMessage>| {
                                match received {
                                    Ok(Message::Text(message)) => {
                                        let message: WebSocketMessage =
                                            serde_json::from_str(&message).unwrap();
                                        match message {
                                            WebSocketMessage::UserMessage(message) => (
                                                Some(Event::MessageReceived(
                                                    WebSocketMessage::UserMessage(message),
                                                )),
                                                State::Connected(websocket, input),
                                            ),
                                        }
                                    }
                                    Ok(_) => (None, State::Connected(websocket, input)),
                                    Err(_) => (Some(Event::Disconnected), State::Disconnected),
                                }
                            };
                        let on_received_user_input =
                            |message,
                             mut websocket: WebSocketStream<MaybeTlsStream<TcpStream>>,
                             input| async move {
                                let message = match message {
                                    Some(message) => message,
                                    None => {
                                        return (Some(Event::Disconnected), State::Disconnected);
                                    }
                                };
                                let message = serde_json::to_string(&message).unwrap();
                                let result = websocket.send(Message::Text(message)).await;

                                if result.is_ok() {
                                    (None, State::Connected(websocket, input))
                                } else {
                                    (Some(Event::Disconnected), State::Disconnected)
                                }
                            };
                        tokio::select! {
                            received = fused_websocket.select_next_some() => {
                                on_receive_remote(received,websocket,input)
                            }

                            message = input.recv() => {
                                on_received_user_input(message,websocket,input).await

                            }
                        }
                    }
                }
            }
        },
    )
}
#[derive(Debug)]
enum State {
    Disconnected,
    Connected(
        WebSocketStream<MaybeTlsStream<TcpStream>>,
        Receiver<WebSocketMessage>,
    ),
}

#[derive(Debug, Clone)]
pub enum Event {
    Connected(Connection),
    Disconnected,
    MessageReceived(WebSocketMessage),
}

#[derive(Debug, Clone)]
pub struct Connection(Sender<WebSocketMessage>);

impl Connection {
    pub async fn send(
        &mut self,
        message: WebSocketMessage,
    ) -> Result<(), tokio::sync::mpsc::error::TrySendError<WebSocketMessage>> {
        self.0.try_send(message)
    }
}
