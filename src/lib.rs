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

pub fn connect() -> Subscription<Event> {
    struct Connect;
    subscription::unfold(
        std::any::TypeId::of::<Connect>(),
        State::WaitingUrl,
        move |state| {
            async move {
                match state {
                    State::Stoped(mut receiver) => {
                        let url = receiver.recv().await.unwrap();
                        (None, State::Disconnected(url))
                    }
                    State::WaitingUrl => {
                        let (sender, receiver) = tokio::sync::mpsc::channel(10);
                        (Some(Event::ReadyToConnect(sender)), State::Stoped(receiver))
                    }
                    State::Disconnected(url) => {
                        match tokio_tungstenite::connect_async(&url).await {
                            Ok((websocket, _)) => {
                                let (sender, receiver) = tokio::sync::mpsc::channel(10);

                                (
                                    Some(Event::Connected(Connection(sender))),
                                    State::Connected(websocket, receiver, url),
                                )
                            }
                            Err(_) => {
                                // Wait for 1 second before retrying
                                println!("Connection failed... Retrying...");
                                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

                                (Some(Event::Disconnected), State::Disconnected(url))
                            }
                        }
                    }
                    State::Connected(mut websocket, mut input, url) => {
                        let mut fused_websocket = websocket.by_ref().fuse();
                        let on_receive_remote =
                            |received,
                             websocket: WebSocketStream<MaybeTlsStream<TcpStream>>,
                             input: Receiver<WebSocketMessage>,
                             url| {
                                match received {
                                    Ok(Message::Text(message)) => {
                                        let message: WebSocketMessage =
                                            serde_json::from_str(&message).unwrap();
                                        match message {
                                            WebSocketMessage::UserMessage(message) => (
                                                Some(Event::MessageReceived(
                                                    WebSocketMessage::UserMessage(message),
                                                )),
                                                State::Connected(websocket, input, url),
                                            ),
                                        }
                                    }
                                    Ok(_) => (None, State::Connected(websocket, input, url)),
                                    Err(_) => (Some(Event::Disconnected), State::Disconnected(url)),
                                }
                            };
                        let on_received_user_input =
                            |message,
                             mut websocket: WebSocketStream<MaybeTlsStream<TcpStream>>,
                             input,
                             url| async move {
                                let message = match message {
                                    Some(message) => message,
                                    None => {
                                        return (
                                            Some(Event::Disconnected),
                                            State::Disconnected(url),
                                        );
                                    }
                                };
                                let message = serde_json::to_string(&message).unwrap();
                                let result = websocket.send(Message::Text(message)).await;

                                if result.is_ok() {
                                    (None, State::Connected(websocket, input, url))
                                } else {
                                    (Some(Event::Disconnected), State::Disconnected(url))
                                }
                            };
                        tokio::select! {
                            received = fused_websocket.select_next_some() => {
                                on_receive_remote(received,websocket,input,url)
                            }

                            message = input.recv() => {
                                on_received_user_input(message,websocket,input,url).await

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
    Stoped(Receiver<String>),
    Disconnected(String),
    Connected(
        WebSocketStream<MaybeTlsStream<TcpStream>>,
        Receiver<WebSocketMessage>,
        String,
    ),
}

#[derive(Debug, Clone)]
pub enum Event {
    ReadyToConnect(Sender<String>),
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
