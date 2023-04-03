#![windows_subsystem = "windows"]
use std::collections::{BTreeSet, VecDeque};
use std::process;

use clap::Parser;
use iced::clipboard;
use iced::keyboard::KeyCode;
use iced::widget::{button, column, row, scrollable, text, text_input};
use iced::{Alignment, Application, Color, Element, Length, Settings};
use tokio::sync::mpsc::Sender;
use tracing::info;
use websocket_chatroom::{
    Connection, MessageData, WebSocketClientToServerMessage, WebSocketServerToClientMessage,
};
#[derive(Parser)]
struct Cli {
    #[clap(short, long)]
    socket_addr: Option<String>,
}

pub fn main() -> eyre::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::builder()
                .with_default_directive("warn".parse().unwrap())
                .from_env_lossy(),
        )
        .with_ansi(true)
        .try_init()
        .unwrap_or_else(|e| {
            eprintln!("failed to init logger: {}", e);
        });

    let cli = Cli::parse();
    let socket_addr = cli
        .socket_addr
        .unwrap_or("wss://chat.thesjq.com".to_string())
        .parse()?;
    let mut settings = Settings::with_flags(socket_addr);
    settings.default_font = Some(include_bytes!("../../assets/XiaoXiangjiaoFont-2OXpK.ttf"));
    ChatRoom::run(settings)?;
    Ok(())
}

enum ConnectionStatus {
    Disconnected,
    Connected {
        connection: Connection,
        input_message: String,
        user_id: u32,
        all_users: BTreeSet<(u32, String)>,
    },
}
enum Page {
    /// the sender to send the url
    Welcome(Sender<(String, String)>),
    Main {
        connections_status: ConnectionStatus,
        message_queue: VecDeque<(bool, MessageData)>,
        log_queue: VecDeque<String>,
    },
}

enum AppStatus {
    /// waiting for the subscription to be ready
    WaitingSubscribtion,

    /// data: the url sender, the user name
    SubReady {
        /// the page type
        page: Page,
    },
}

struct ChatRoom {
    app_status: AppStatus,
    user_name: String,
    url: String,
}

#[derive(Debug, Clone)]
enum Message {
    /// the sender send url and user name
    EnterWelcome(Sender<(String, String)>),
    EnterMain,
    Connected(Connection, u32, Vec<(u32, String)>),
    Disconnected(String),
    MessageReceived(WebSocketServerToClientMessage),
    InputChange(String),
    UserNameChange(String),
    UrlChange(String),
    Copy(String),
    Send,
    Sent,
    Clear,
    Exit,
}

impl Application for ChatRoom {
    type Message = Message;

    type Executor = iced::executor::Default;

    type Theme = iced::Theme;

    type Flags = String;

    fn new(socket_addr: Self::Flags) -> (Self, iced::Command<Self::Message>) {
        (
            Self {
                app_status: AppStatus::WaitingSubscribtion,
                user_name: "Guest".to_string(),
                url: socket_addr,
            },
            iced::Command::none(),
        )
    }

    fn title(&self) -> String {
        String::from("chatroom")
    }

    fn update(&mut self, message: Message) -> iced::Command<Message> {
        match message {
            Message::EnterWelcome(url_sender) => {
                self.app_status = AppStatus::SubReady {
                    page: Page::Welcome(url_sender),
                };
                iced::Command::none()
            }
            Message::EnterMain => {
                if let AppStatus::SubReady { page } = &mut self.app_status {
                    match page {
                        Page::Welcome(sender) => {
                            sender
                                .try_send((self.url.clone(), self.user_name.clone()))
                                .unwrap();
                            *page = Page::Main {
                                connections_status: ConnectionStatus::Disconnected,
                                message_queue: VecDeque::new(),
                                log_queue: VecDeque::new(),
                            };
                        }
                        _ => {}
                    }

                    iced::Command::none()
                } else {
                    iced::Command::none()
                }
            }
            Message::Connected(connection, user_id, all_users) => {
                if let AppStatus::SubReady {
                    page:
                        Page::Main {
                            connections_status, ..
                        },
                } = &mut self.app_status
                {
                    *connections_status = ConnectionStatus::Connected {
                        connection,
                        input_message: String::new(),
                        user_id,
                        all_users: all_users.into_iter().collect(),
                    };
                }
                iced::Command::none()
            }
            Message::Disconnected(_error_message) => {
                if let AppStatus::SubReady {
                    page:
                        Page::Main {
                            connections_status, ..
                        },
                } = &mut self.app_status
                {
                    *connections_status = ConnectionStatus::Disconnected;
                }
                iced::Command::none()
            }
            Message::MessageReceived(message) => {
                if let AppStatus::SubReady {
                    page:
                        Page::Main {
                            message_queue,
                            connections_status,
                            ..
                        },
                } = &mut self.app_status
                {
                    match connections_status {
                        ConnectionStatus::Disconnected => {}
                        ConnectionStatus::Connected { all_users, .. } => match message {
                            WebSocketServerToClientMessage::UserMessage(message) => {
                                info!("message: {:?}", message);
                                message_queue.push_back((false, message))
                            }
                            WebSocketServerToClientMessage::Disconnected(id, name) => {
                                info!("message disconnected: {:?} {}", id, name);
                                all_users.remove(&(id, name));
                            }
                            WebSocketServerToClientMessage::NewUserAdded(id, name) => {
                                info!("message new user: {:?} {}", id, name);
                                all_users.insert((id, name));
                            }
                            _ => {}
                        },
                    }
                }
                iced::Command::none()
            }
            Message::InputChange(input) => {
                if let AppStatus::SubReady {
                    page:
                        Page::Main {
                            connections_status: ConnectionStatus::Connected { input_message, .. },
                            ..
                        },
                } = &mut self.app_status
                {
                    *input_message = input;
                }
                iced::Command::none()
            }
            Message::UrlChange(url) => {
                self.url = url;
                iced::Command::none()
            }
            Message::UserNameChange(user_name) => {
                self.user_name = user_name;
                iced::Command::none()
            }
            Message::Send => {
                if let AppStatus::SubReady {
                    page:
                        Page::Main {
                            connections_status:
                                ConnectionStatus::Connected {
                                    connection,
                                    input_message,
                                    user_id,
                                    ..
                                },
                            message_queue,
                            ..
                        },
                } = &mut self.app_status
                {
                    let data = MessageData {
                        id: *user_id,
                        name: self.user_name.clone(),
                        data: input_message.clone(),
                    };
                    let message = WebSocketClientToServerMessage::UserMessage(data.clone());

                    message_queue.push_back((true, data));
                    let mut connection = connection.clone();
                    iced::Command::perform(
                        async move {
                            connection
                                .send(message)
                                .await
                                .map_err(|_| "cannot send to sub")?;
                            Ok(())
                        },
                        |result: Result<_, &str>| {
                            if result.is_err() {
                                Message::Disconnected(result.err().unwrap().to_string())
                            } else {
                                Message::Sent
                            }
                        },
                    )
                } else {
                    iced::Command::none()
                }
            }
            Message::Sent => {
                if let AppStatus::SubReady {
                    page:
                        Page::Main {
                            connections_status: ConnectionStatus::Connected { input_message, .. },
                            log_queue,
                            ..
                        },
                } = &mut self.app_status
                {
                    *input_message = String::new();
                    log_queue.push_back("sent".to_string());
                }
                iced::Command::none()
            }
            Message::Clear => {
                if let AppStatus::SubReady {
                    page:
                        Page::Main {
                            log_queue,
                            message_queue,
                            ..
                        },
                } = &mut self.app_status
                {
                    log_queue.clear();
                    message_queue.clear();
                }
                iced::Command::none()
            }
            Message::Exit => {
                process::exit(0);
            }
            Message::Copy(text) => clipboard::write(text),
        }
    }

    fn subscription(&self) -> iced::Subscription<Self::Message> {
        let web_socket_sub = websocket_chatroom::connect().map(|event| match event {
            websocket_chatroom::Event::Connected(sender, id, all_users) => {
                Message::Connected(sender, id, all_users)
            }
            websocket_chatroom::Event::Disconnected => {
                Message::Disconnected("Disconnected".to_string())
            }
            websocket_chatroom::Event::MessageReceived(message) => {
                Message::MessageReceived(message)
            }
            websocket_chatroom::Event::ReadyToConnect(url_sender) => {
                // enter the welcome stat
                Message::EnterWelcome(url_sender)
            }
        });
        let func: fn(iced::Event, iced::event::Status) -> Option<Message> =
            |event, _status| match event {
                iced::Event::Keyboard(key) => match key {
                    iced::keyboard::Event::KeyReleased {
                        key_code,
                        modifiers: _,
                    } if key_code == KeyCode::Enter => Some(Message::Send),
                    _ => None,
                },
                _ => None,
            };
        let key_board_sub = iced::subscription::events_with(func);
        iced::Subscription::batch(vec![web_socket_sub, key_board_sub])
    }

    fn view(&self) -> Element<Message> {
        match &self.app_status {
            AppStatus::WaitingSubscribtion => text("Waiting for subscribtion init").size(20).into(),
            AppStatus::SubReady { page } => match page {
                Page::Welcome(_) => self.welcome_view(),
                Page::Main {
                    connections_status,
                    message_queue,
                    log_queue,
                } => match connections_status {
                    ConnectionStatus::Disconnected => {
                        self.disconnected_view(message_queue, log_queue)
                    }
                    ConnectionStatus::Connected {
                        input_message,
                        user_id,
                        all_users,
                        ..
                    } => self.connected_view(
                        message_queue,
                        log_queue,
                        input_message,
                        *user_id,
                        all_users.iter(),
                    ),
                },
            },
        }
    }
}

impl ChatRoom {
    fn welcome_view(&self) -> Element<Message> {
        let user_name = text_input("user name", &self.user_name, |msg| {
            Message::UserNameChange(msg)
        });
        let url = text_input("url", &self.url, |msg| Message::UrlChange(msg));
        let start_bt = button("start").padding(5).on_press(Message::EnterMain);
        let col = column(vec![user_name.into(), url.into(), start_bt.into()])
            .align_items(Alignment::Center)
            .padding(10)
            .width(Length::Fill)
            .height(Length::Fill);
        col.into()
    }

    fn disconnected_view(
        &self,
        message_queue: &VecDeque<(bool, MessageData)>,
        log_queue: &VecDeque<String>,
    ) -> Element<Message> {
        let text = text("Disconnected")
            .size(20)
            .style(Color::from_rgb8(102, 102, 153));
        let msg_log_row = build_msg_and_log(message_queue, log_queue);
        let col = column(vec![text.into(), msg_log_row.into()])
            .align_items(Alignment::Center)
            .padding(10)
            .width(Length::Fill)
            .height(Length::Fill);
        col.into()
    }

    fn connected_view<'a>(
        &self,
        message_queue: &VecDeque<(bool, MessageData)>,
        log_queue: &VecDeque<String>,
        input_message: &str,
        user_id: u32,
        all_users: impl IntoIterator<Item = &'a (u32, String)>,
    ) -> Element<Message> {
        let status = format!("Connected: id: {user_id}, name: {}", self.user_name);
        let status_text = text(status).size(20).style(Color::from_rgb8(102, 102, 153));

        let send_bt = button("send").padding(5).on_press(Message::Send);
        let exit_bt = button("exit").padding(5).on_press(Message::Exit);
        let clear_bt = button("clear").padding(5).on_press(Message::Clear);
        let bt_row = row(vec![send_bt.into(), exit_bt.into(), clear_bt.into()])
            .padding(10)
            .spacing(3)
            .align_items(Alignment::Center);
        let input_message =
            text_input("input here", input_message, |msg| Message::InputChange(msg));

        let msg_log_row = build_msg_and_log(message_queue, log_queue);
        let all_connected_users: String = all_users
            .into_iter()
            .map(|user| format!("{}-{}", user.0, user.1))
            .fold(String::new(), |mut f, s| {
                f.push_str(&s);
                f.push_str(" ");
                f
            });

        info!("all users: {:?}", all_connected_users.len());

        let col = column(vec![
            status_text.into(),
            text(format!("all users:")).into(),
            text(all_connected_users).into(),
            bt_row.into(),
            input_message.into(),
            msg_log_row.into(),
        ])
        .align_items(Alignment::Center)
        .padding(10)
        .width(Length::Fill)
        .height(Length::Fill);
        col.into()
    }
}

fn build_msg_and_log(
    message_queue: &VecDeque<(bool, MessageData)>,
    log_queue: &VecDeque<String>,
) -> Element<'static, Message> {
    let chat_messages = message_queue
        .iter()
        .map(|msg| {
            let data = &msg.1;
            let text = text(format!("{}: {}", data.name, data.data)).size(20);

            let text = if msg.0 {
                text.style(Color::from_rgb8(204, 51, 0))
            } else {
                text.style(Color::from_rgb8(0, 51, 102))
            };
            let copy_bt = button("copy").on_press(Message::Copy(data.data.clone()));
            row(vec![text.into(), copy_bt.into()])
                .align_items(Alignment::Center)
                .padding(5)
                .into()
        })
        .collect();
    let logs = log_queue
        .iter()
        .map(|msg| {
            text(msg)
                .size(20)
                .style(Color::from_rgb8(0, 51, 102))
                .into()
        })
        .collect();
    let msg_col = scrollable(
        column(chat_messages)
            .spacing(15)
            .align_items(Alignment::Start)
            .width(Length::FillPortion(8))
            .padding(15),
    )
    .height(Length::Fill);
    let log_col = scrollable(
        column(logs)
            .spacing(15)
            .align_items(Alignment::End)
            .width(Length::FillPortion(2))
            .padding(15),
    )
    .height(Length::Fill);
    let msg_log_row = row(vec![msg_col.into(), log_col.into()])
        .spacing(15)
        .align_items(Alignment::Start)
        .width(Length::Fill);
    msg_log_row.into()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    #[tokio::test(flavor = "current_thread")]
    async fn test() {
        let client = reqwest::ClientBuilder::new()
            .tcp_keepalive(Duration::from_secs(60))
            .build()
            .unwrap();
        let resp = client.get("http://www.baidu.com").send().await.unwrap();
        println!("{:?}", resp.text().await.unwrap());
    }
}
