use std::collections::VecDeque;

use clap::Parser;
use iced::widget::{button, column, row, scrollable, text, text_input};
use iced::{Alignment, Application, Color, Element, Length, Settings};
use websocket_chatroom::{Connection, MessageData, WebSocketMessage};
#[derive(Parser)]
struct Cli {
    #[clap(short, long)]
    socket_addr: Option<String>,
}

pub fn main() -> eyre::Result<()> {
    let cli = Cli::parse();
    let socket_addr = cli
        .socket_addr
        .unwrap_or("ws://127.0.0.1:2233".to_string())
        .parse()?;
    println!("socket_addr: {:?}", socket_addr);
    let mut settings = Settings::with_flags(socket_addr);
    settings.default_font = Some(include_bytes!("../../assets/XiaoXiangjiaoFont-2OXpK.ttf"));
    ChatRoom::run(settings)?;
    Ok(())
}

enum AppStatus {
    Disconnected,
    Connected(Connection, u32, String),
}
struct ChatRoom {
    server_url: String,
    app_status: AppStatus,
    current_message: Option<String>,
    message_queue: VecDeque<(bool, WebSocketMessage)>,
    log_queue: VecDeque<String>,
    input_message: String,
    msg_id: u32,
}

#[derive(Debug, Clone)]
enum Message {
    /// the sender and the user id
    Connected(Connection, u32),
    Disconnected(String),
    MessageReceived(WebSocketMessage),
    InputChange(String),
    Send(String),
    Sent,
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
                server_url: socket_addr,
                app_status: AppStatus::Disconnected,
                current_message: None,
                message_queue: VecDeque::new(),
                input_message: String::new(),
                log_queue: VecDeque::new(),
                msg_id: 0,
            },
            iced::Command::none(),
        )
    }

    fn title(&self) -> String {
        String::from("chatroom")
    }

    fn update(&mut self, message: Message) -> iced::Command<Message> {
        match message {
            Message::Connected(sender, user_id) => {
                self.app_status = AppStatus::Connected(sender, user_id, "ppsjq".to_string());
                self.current_message = Some("Connected".to_string());
                iced::Command::none()
            }
            Message::Disconnected(error_message) => {
                self.app_status = AppStatus::Disconnected;
                self.current_message = Some(error_message);
                iced::Command::none()
            }
            Message::MessageReceived(msg) => {
                self.message_queue.push_back((false, msg));
                if self.message_queue.len() > 100 {
                    self.message_queue.pop_front();
                }
                iced::Command::none()
            }

            Message::Exit => {
                std::process::exit(0);
            }
            Message::Send(msg) => match &self.app_status {
                AppStatus::Disconnected => {
                    self.current_message = Some("Disconnected, cannot send".to_string());
                    iced::Command::none()
                }
                AppStatus::Connected(connection, id, name) => {
                    let msg = WebSocketMessage::UserMessage(MessageData {
                        id: *id,
                        name: name.clone(),
                        data: msg,
                    });
                    self.msg_id += 1;
                    self.message_queue.push_back((true, msg.clone()));
                    if self.message_queue.len() > 100 {
                        self.message_queue.pop_front();
                    }
                    self.log_queue.push_back(format!("Sending {}", self.msg_id));
                    let mut sender = connection.clone();
                    iced::Command::perform(async move { sender.send(msg).await }, |res| match res {
                        Ok(_) => Message::Sent,
                        Err(_e) => Message::Exit,
                    })
                }
            },
            Message::Sent => {
                self.log_queue.push_back(format!("Sent {}", self.msg_id));
                if self.log_queue.len() > 100 {
                    self.log_queue.pop_front();
                }
                iced::Command::none()
            }
            Message::InputChange(input) => {
                self.input_message = input;
                iced::Command::none()
            }
        }
    }

    fn subscription(&self) -> iced::Subscription<Self::Message> {
        websocket_chatroom::connect(&self.server_url).map(|event| match event {
            websocket_chatroom::Event::Connected(sender) => Message::Connected(sender, 0),
            websocket_chatroom::Event::Disconnected => {
                Message::Disconnected("Disconnected".to_string())
            }
            websocket_chatroom::Event::MessageReceived(message) => {
                Message::MessageReceived(message)
            }
        })
    }

    fn view(&self) -> Element<Message> {
        let status = match &self.app_status {
            AppStatus::Disconnected => format!("Disconnected, server_id: {}", self.server_url),
            AppStatus::Connected(_, id, name) => format!("Connected: id: {id}, name: {name}"),
        };
        let status_text = text(status).size(20).style(Color::from_rgb8(102, 102, 153));

        let send_bt = button("send")
            .padding(5)
            .on_press(Message::Send(self.input_message.clone()));
        let exit_bt = button("exit").padding(5).on_press(Message::Exit);
        let bt_row = row(vec![send_bt.into(), exit_bt.into()])
            .padding(10)
            .spacing(3)
            .align_items(Alignment::Center);
        let input_message = text_input("input here", &self.input_message, |msg| {
            Message::InputChange(msg)
        });
        let current_message = self
            .current_message
            .as_ref()
            .map(|msg| scrollable(text(msg).size(20).style(Color::from_rgb8(0, 51, 102))))
            .unwrap_or_else(|| {
                scrollable(
                    text("No message")
                        .size(20)
                        .style(Color::from_rgb8(204, 51, 0)),
                )
            });
        let chat_messages = self
            .message_queue
            .iter()
            .map(|msg| match &msg.1 {
                WebSocketMessage::UserMessage(data) => {
                    let text = text(format!("{}: {}", data.name, data.data)).size(20);

                    let text = if msg.0 {
                        text.style(Color::from_rgb8(204, 51, 0))
                    } else {
                        text.style(Color::from_rgb8(0, 51, 102))
                    };
                    text.into()
                }
            })
            .collect();
        let logs = self
            .log_queue
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
                .spacing(5)
                .align_items(Alignment::Start),
        )
        .height(Length::FillPortion(8));
        let log_col = scrollable(column(logs).spacing(5).align_items(Alignment::Start))
            .height(Length::FillPortion(2));
        let msg_log_row = row(vec![msg_col.into(), log_col.into()])
            .spacing(5)
            .align_items(Alignment::Start);
        let col = column(vec![
            status_text.into(),
            bt_row.into(),
            input_message.into(),
            current_message.into(),
            msg_log_row.into(),
        ])
        .align_items(Alignment::Center)
        .padding(10)
        .width(Length::Fill)
        .height(Length::Fill);
        col.into()
    }
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
