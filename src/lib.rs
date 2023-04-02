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
pub enum WebSocketClientToServerMessage {
    UserMessage(MessageData),
    Connect(String),
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum WebSocketServerToClientMessage {
    UserMessage(MessageData),
    /// self connect success
    Connected(u32, String),
    /// other user connect
    NewUserAdded(u32, String),
    /// other user disconnect
    Disconnected(u32, String),
    /// all users
    AllUsers(Vec<(u32, String)>),
}

pub fn connect() -> Subscription<Event> {
    struct Connect;
    subscription::unfold(
        std::any::TypeId::of::<Connect>(),
        State::WaitingUrl,
        move |state| {
            async move {
                match state {
                    State::Stoped(mut receiver) => {
                        let (url, user_name) = receiver.recv().await.unwrap();
                        (None, State::Disconnected(url, user_name))
                    }
                    State::WaitingUrl => {
                        let (sender, receiver) = tokio::sync::mpsc::channel(10);
                        (Some(Event::ReadyToConnect(sender)), State::Stoped(receiver))
                    }
                    State::Disconnected(url, user_name) => {
                        match tokio_tungstenite::connect_async(&url).await {
                            Ok((mut websocket, _)) => {
                                let (sender, receiver) = tokio::sync::mpsc::channel(10);
                                // send the connect message to server
                                let message = WebSocketClientToServerMessage::Connect(user_name);
                                let message = serde_json::to_string(&message).unwrap();
                                websocket.send(Message::Text(message)).await.unwrap();
                                // receive the id from server
                                let (id, user_name) = match websocket.next().await {
                                    Some(Ok(Message::Text(message))) => {
                                        let message: WebSocketServerToClientMessage =
                                            serde_json::from_str(&message).unwrap();
                                        match message {
                                            WebSocketServerToClientMessage::Connected(
                                                id,
                                                user_name,
                                            ) => (id, user_name),
                                            _ => panic!("Unexpected message"),
                                        }
                                    }
                                    _ => panic!("Unexpected message"),
                                };
                                let all_users = match websocket.next().await {
                                    Some(Ok(Message::Text(message))) => {
                                        let message: WebSocketServerToClientMessage =
                                            serde_json::from_str(&message).unwrap();
                                        match message {
                                            WebSocketServerToClientMessage::AllUsers(all_users) => {
                                                all_users
                                            }
                                            _ => panic!("Unexpected message"),
                                        }
                                    }
                                    _ => panic!("Unexpected message"),
                                };
                                (
                                    Some(Event::Connected(Connection(sender), id, all_users)),
                                    State::Connected(websocket, receiver, url, user_name),
                                )
                            }
                            Err(_) => {
                                // Wait for 1 second before retrying
                                println!("Connection failed... Retrying...");
                                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

                                (
                                    Some(Event::Disconnected),
                                    State::Disconnected(url, user_name),
                                )
                            }
                        }
                    }
                    State::Connected(mut websocket, mut input, url, user_name) => {
                        let mut fused_websocket = websocket.by_ref().fuse();
                        let on_receive_remote =
                            |received,
                             websocket: WebSocketStream<MaybeTlsStream<TcpStream>>,
                             input: Receiver<WebSocketClientToServerMessage>,
                             url,
                             user_name| {
                                match received {
                                    Ok(Message::Text(message)) => {
                                        let message: WebSocketServerToClientMessage =
                                            serde_json::from_str(&message).unwrap();
                                        (
                                            Some(Event::MessageReceived(message)),
                                            State::Connected(websocket, input, url, user_name),
                                        )
                                    }
                                    Ok(_) => {
                                        (None, State::Connected(websocket, input, url, user_name))
                                    }
                                    Err(_) => (
                                        Some(Event::Disconnected),
                                        State::Disconnected(url, user_name),
                                    ),
                                }
                            };
                        let on_received_user_input =
                            |message,
                             mut websocket: WebSocketStream<MaybeTlsStream<TcpStream>>,
                             input,
                             url,
                             user_name| async move {
                                let message = match message {
                                    Some(message) => message,
                                    None => {
                                        return (
                                            Some(Event::Disconnected),
                                            State::Disconnected(url, user_name),
                                        );
                                    }
                                };
                                let message = serde_json::to_string(&message).unwrap();
                                let result = websocket.send(Message::Text(message)).await;

                                if result.is_ok() {
                                    (None, State::Connected(websocket, input, url, user_name))
                                } else {
                                    (
                                        Some(Event::Disconnected),
                                        State::Disconnected(url, user_name),
                                    )
                                }
                            };
                        tokio::select! {
                            received = fused_websocket.select_next_some() => {
                                on_receive_remote(received,websocket,input,url,user_name)
                            }

                            message = input.recv() => {
                                on_received_user_input(message,websocket,input,url,user_name).await

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
    WaitingUrl,
    Stoped(Receiver<(String, String)>),
    Disconnected(String, String),
    Connected(
        WebSocketStream<MaybeTlsStream<TcpStream>>,
        Receiver<WebSocketClientToServerMessage>,
        String,
        String,
    ),
}

#[derive(Debug, Clone)]
pub enum Event {
    ReadyToConnect(Sender<(String, String)>),
    Connected(Connection, u32, Vec<(u32, String)>),
    Disconnected,
    MessageReceived(WebSocketServerToClientMessage),
}

#[derive(Debug, Clone)]
pub struct Connection(Sender<WebSocketClientToServerMessage>);

impl Connection {
    pub async fn send(
        &mut self,
        message: WebSocketClientToServerMessage,
    ) -> Result<(), tokio::sync::mpsc::error::TrySendError<WebSocketClientToServerMessage>> {
        self.0.try_send(message)
    }
}

#[cfg(test)]
mod tests {
    enum E {
        A(String),
        B(String),
    }
    #[test]
    fn test_enum() {
        let mut e = E::A("a".to_string());

        change_e(&mut e);
    }

    fn change_e(e: &mut E) {
        // swap a and b
        match e {
            E::A(a) => {
                *e = E::B(a.clone());
            }
            E::B(b) => {
                *e = E::A(b.clone());
            }
        }
    }
}
