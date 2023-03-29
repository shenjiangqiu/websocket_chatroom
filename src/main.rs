use std::net::SocketAddr;
use std::time::Duration;

use clap::Parser;
use iced::widget::{button, column, row, scrollable, text};
use iced::{Alignment, Application, Color, Element, Length, Settings};
#[derive(Parser)]
struct Cli {
    #[clap(short, long)]
    socket_addr: Option<String>,
}

pub fn main() -> eyre::Result<()> {
    let cli = Cli::parse();
    let socket_addr = cli
        .socket_addr
        .unwrap_or("127.0.0.1:2233".to_string())
        .parse()?;
    println!("socket_addr: {:?}", socket_addr);
    let mut settings = Settings::with_flags(socket_addr);
    settings.default_font = Some(include_bytes!("../assets/XiaoXiangjiaoFont-2OXpK.ttf"));
    SpmspmMonitor::run(settings)?;
    Ok(())
}

enum AppStatus {
    Disconnected,
    Connecting,
    Connected(bool, reqwest::Client),
}
struct SpmspmMonitor {
    server_id: SocketAddr,
    app_status: AppStatus,
    current_message: Option<String>,
}

#[derive(Debug, Clone)]
enum Message {
    Connected(reqwest::Client, String),
    Disconnected(String),
    Connecting,
    MessageReceived(String, bool),
    ToggleAutoRefresh,
    Refresh,
    Exit,
}

impl Application for SpmspmMonitor {
    type Message = Message;

    type Executor = iced::executor::Default;

    type Theme = iced::Theme;

    type Flags = SocketAddr;
    fn new(socket_addr: Self::Flags) -> (Self, iced::Command<Self::Message>) {
        (
            Self {
                server_id: socket_addr,
                app_status: AppStatus::Disconnected,
                current_message: None,
            },
            iced::Command::none(),
        )
    }

    fn title(&self) -> String {
        String::from("Counter - Iced")
    }

    fn update(&mut self, message: Message) -> iced::Command<Message> {
        match message {
            Message::Connected(client, first_message) => {
                self.app_status = AppStatus::Connected(false, client);
                self.current_message = Some(first_message);
                iced::Command::none()
            }
            Message::Disconnected(error_message) => {
                self.app_status = AppStatus::Disconnected;
                self.current_message = Some(error_message);
                iced::Command::none()
            }
            Message::MessageReceived(msg, is_auto_refresh) => match &self.app_status {
                AppStatus::Connected(auto_refresh, client) => {
                    self.current_message = Some(msg);

                    let client = client.clone();
                    if *auto_refresh && is_auto_refresh {
                        iced::Command::perform(
                            async move {
                                tokio::time::sleep(Duration::from_secs(1)).await;
                                let msg = client
                                    .get("http://www.google.com")
                                    .send()
                                    .await
                                    .map_err(|e| e.to_string())?
                                    .text()
                                    .await
                                    .map_err(|e| e.to_string())?;
                                Ok(msg)
                            },
                            |result: Result<String, String>| match result {
                                Ok(msg) => Message::MessageReceived(msg, true),
                                Err(e) => Message::Disconnected(e),
                            },
                        )
                    } else {
                        iced::Command::none()
                    }
                }
                _ => iced::Command::none(),
            },
            Message::Connecting => {
                self.app_status = AppStatus::Connecting;
                iced::Command::perform(
                    async move {
                        let client = reqwest::ClientBuilder::new()
                            .tcp_keepalive(std::time::Duration::from_secs(60))
                            .build()
                            .map_err(|e| e.to_string())?;
                        let first_message = client
                            .get("http://www.baidu.com")
                            .send()
                            .await
                            .map_err(|e| e.to_string())?
                            .text()
                            .await
                            .map_err(|e| e.to_string())?;
                        Ok((client, first_message))
                    },
                    |result: Result<_, String>| match result {
                        Ok((client, first_message)) => Message::Connected(client, first_message),
                        Err(err_message) => Message::Disconnected(err_message),
                    },
                )
            }
            Message::ToggleAutoRefresh => match &mut self.app_status {
                AppStatus::Connected(auto_refresh, _) => {
                    *auto_refresh = !*auto_refresh;
                    iced::Command::perform(async move {}, |_| {
                        Message::MessageReceived("Start auto refresh".to_string(), true)
                    })
                }
                _ => {
                    self.current_message = Some("Not connected".to_string());
                    iced::Command::none()
                }
            },
            Message::Refresh => match &mut self.app_status {
                AppStatus::Connected(_, client) => {
                    let client = client.clone();
                    iced::Command::perform(
                        async move {
                            let message = client
                                .get("http://www.google.com")
                                .send()
                                .await
                                .map_err(|e| e.to_string())?
                                .text()
                                .await
                                .map_err(|e| e.to_string())?;
                            Ok(message)
                        },
                        |result: Result<_, String>| match result {
                            Ok(message) => Message::MessageReceived(message, false),
                            Err(err_message) => Message::Disconnected(err_message),
                        },
                    )
                }
                _ => {
                    self.current_message = Some("Not connected".to_string());
                    iced::Command::none()
                }
            },
            Message::Exit => {
                std::process::exit(0);
            }
        }
    }

    fn view(&self) -> Element<Message> {
        let status = match self.app_status {
            AppStatus::Disconnected => format!("Disconnected, server_id: {}", self.server_id),
            AppStatus::Connected(refresh, _) => format!("Connected: refresh: {refresh}"),
            AppStatus::Connecting => "Connecting...".to_string(),
        };
        let status_text = text(status).size(20).style(Color::from_rgb8(102, 102, 153));
        let connect_bt = button("connect").on_press(Message::Connecting).padding(5);
        let toggle_refresh_bt = button("toggle_refresh")
            .on_press(Message::ToggleAutoRefresh)
            .padding(5);
        let refresh_bt = button("refresh").on_press(Message::Refresh).padding(5);
        let disconnect_bt = button("disconnect")
            .padding(5)
            .on_press(Message::Disconnected("Disconnected by user".to_string()));
        let exit_bt = button("exit").padding(5).on_press(Message::Exit);
        let row = row![
            connect_bt,
            toggle_refresh_bt,
            refresh_bt,
            disconnect_bt,
            exit_bt
        ]
        .padding(10)
        .spacing(3)
        .align_items(Alignment::Center);
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
        let col = column(vec![status_text.into(), row.into(), current_message.into()])
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
