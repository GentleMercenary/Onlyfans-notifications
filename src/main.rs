#![windows_subsystem = "windows"]
#![feature(result_option_inspect)]

mod client;
mod message_types;
mod websocket_client;
use client::ClientExt;
use message_types::Error;

#[macro_use]
extern crate log;
extern crate simplelog;

use cached::once_cell::sync::OnceCell;
use chrono::Local;
use futures::TryFutureExt;
use reqwest::Client;
use simplelog::{Config, LevelFilter, WriteLogger};
use std::{
	error,
	fs::{self, File},
	path::Path,
	sync::Arc,
};
use tokio::{select, task};
use tokio_util::sync::CancellationToken;
use trayicon::{Icon, MenuBuilder, TrayIconBuilder};
use winit::{
	event::Event,
	event_loop::{ControlFlow, EventLoop, EventLoopProxy},
};
use winrt_toast::{register, Toast, ToastManager};

static MANAGER: OnceCell<ToastManager> = OnceCell::new();

fn register_app() -> Result<(), Box<dyn error::Error>> {
	let aum_id = "OFNotifier";
	let icon_path = Path::new("res").join("icon.ico").canonicalize()?; // Doesn't work for some reason
	register(aum_id, "OF noitifier", Some(icon_path.as_path()))?;
	MANAGER
		.set(ToastManager::new(aum_id))
		.expect("Global toast manager set");
	Ok(())
}

fn spawn_connection_thread(proxy: EventLoopProxy<Events>, cancel_token: Arc<CancellationToken>) {
	info!("Spawning websocket thread");
	let cloned_proxy = proxy.clone();
	task::spawn(async move {
		fn on_error(err: Error) {
			error!("Termination caused by: {:?}", err);

			let mut toast = Toast::new();
			toast
				.text1("OF Notifier")
				.text2("An error occurred, disconnecting");

			MANAGER.wait().show(&toast).unwrap();
		}

		let auth_link: &str = "https://onlyfans.com/api2/v2/users/me";
		info!("Fetching authentication parameters");

		Client::with_auth()
			.and_then(|client| async move { client.fetch(auth_link).await })
			.and_then(|response| async move { response.text().await.map_err(|err| err.into()) })
			.and_then(|response| async move {
				info!("Successful fetch for authentication parameters");

				let init_msg: message_types::InitMessage = serde_json::from_str(&response)?;
				debug!("{:?}", init_msg);
				let mut socket = websocket_client::WebSocketClient::new()?;

				let res = socket
					.connect(init_msg.ws_auth_token)
					.and_then(|socket| async move {
						cloned_proxy.send_event(Events::Connected)?;

						loop {
							select! {
								_ = cancel_token.cancelled() => break,
								res = socket.message_loop() => return res
							}
						}

						Ok(())
					})
					.await;

				info!("Terminating websocket");
				socket.close().await?;
				return res;
			})
			.unwrap_or_else(on_error)
			.await;

		info!("Killing websocket thread");
		proxy.send_event(Events::Disconnected).unwrap()
	});
}

#[derive(PartialEq, Eq)]
enum State {
	Disconnected,
	Connecting,
	Connected,
}

#[derive(Clone, Eq, PartialEq, Debug)]
enum Events {
	ClickTrayIcon,
	Connected,
	Disconnected,
	Quit,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn error::Error>> {
	register_app()?;

	fs::create_dir_all(&Path::new("logs"))?;
	let mut log_path = Path::new("logs").join(Local::now().format("%Y%m%d_%H%M%S").to_string());
	log_path.set_extension(".log");

	WriteLogger::init(
		LevelFilter::Info,
		Config::default(),
		File::create(log_path)?,
	)?;

	let event_loop = EventLoop::<Events>::with_user_event();
	let proxy = event_loop.create_proxy();
	let icon = include_bytes!("../res/icon.ico");
	let icon2 = include_bytes!("../res/icon2.ico");

	let first_icon = Icon::from_buffer(icon, None, None)?;
	let second_icon = Icon::from_buffer(icon2, None, None)?;

	let mut tray_icon = TrayIconBuilder::new()
		.sender_winit(proxy.clone())
		.icon_from_buffer(icon2)
		.tooltip("OF notifier")
		.on_click(Events::ClickTrayIcon)
		.menu(MenuBuilder::new().item("Quit", Events::Quit))
		.build()?;

	let mut state = State::Connecting;
	let mut cancel_token = Arc::new(CancellationToken::new());

	spawn_connection_thread(proxy.clone(), cancel_token.clone());
	event_loop.run(move |event, _, control_flow| {
		*control_flow = ControlFlow::Wait;
		let _ = tray_icon;

		match event {
			Event::UserEvent(e) => match e {
				Events::ClickTrayIcon => {
					info!("Tray icon clicked");
					if state == State::Connected {
						info!("Disconnecting");
						cancel_token.cancel();
					} else if state == State::Disconnected {
						cancel_token = Arc::new(CancellationToken::new());
						info!("Connecting");
						state = State::Connecting;
						spawn_connection_thread(proxy.clone(), cancel_token.clone());
					}
				}
				Events::Connected => {
					tray_icon.set_icon(&first_icon).unwrap();
					state = State::Connected;
					info!("Connected");
				}
				Events::Disconnected => {
					tray_icon.set_icon(&second_icon).unwrap();
					state = State::Disconnected;
					info!("Disconnected");
				}
				Events::Quit => {
					info!("Closing application");
					cancel_token.cancel();
					*control_flow = ControlFlow::Exit;
				}
			},
			_ => (),
		}
	});
}

#[cfg(test)]
mod tests {
	use simplelog::{ColorChoice, TermLogger, TerminalMode};

	use super::*;

	#[tokio::test]
	async fn test_chat_message() {
		register_app().unwrap();

		TermLogger::init(
			LevelFilter::Info,
			Config::default(),
			TerminalMode::Mixed,
			ColorChoice::Auto,
		)
		.unwrap();

		let incoming = r#"{
			"api2_chat_message": {
				"text": "This is a message<br />\n to test <a href = \"/onlyfans\">MARKDOWN parsing</a> 👌<br />\n in notifications 💯",
				"price": 3.99,
				"fromUser": {
					"avatar": "https://public.onlyfans.com/files/m/mk/mka/mkamcrf6rjmcwo0jj4zoavhmalzohe5a1640180203/avatar.jpg",
					"id": 15585607,
					"name": "OnlyFans",
					"username": "onlyfans"
				},
				"media": [
					{
						"id": 0,
						"canView": true,
						"src": "https://raw.githubusercontent.com/allenbenz/winrt-notification/main/resources/test/chick.jpeg",
						"preview": "https://raw.githubusercontent.com/allenbenz/winrt-notification/main/resources/test/flower.jpeg",
						"type": "photo"
					}
				]
			} 
		}"#;

		let msg = serde_json::from_str::<message_types::MessageType>(&incoming).unwrap();
		assert!(matches!(
			msg,
			message_types::MessageType::Tagged(message_types::TaggedMessageType::Api2ChatMessage(_))
		));
		msg.handle_message().await.unwrap();
	}

	#[tokio::test]
	async fn test_post_message() {
		register_app().unwrap();

		TermLogger::init(
			LevelFilter::Info,
			Config::default(),
			TerminalMode::Mixed,
			ColorChoice::Auto,
		)
		.unwrap();

		// Onlyfan april fools post
		let incoming = r#"{
			"post_published": {
				"id": "129720708",
				"user_id" : "15585607",
				"show_posts_in_feed":true
			}
		}"#;

		let msg = serde_json::from_str::<message_types::MessageType>(&incoming).unwrap();
		assert!(matches!(
			msg,
			message_types::MessageType::Tagged(message_types::TaggedMessageType::PostPublished(_))
		));
		msg.handle_message().await.unwrap();
	}

	#[tokio::test]
	async fn test_story_message() {
		register_app().unwrap();

		TermLogger::init(
			LevelFilter::Info,
			Config::default(),
			TerminalMode::Mixed,
			ColorChoice::Auto,
		)
		.unwrap();

		let incoming = r#"{
			"stories": [
				{
					"id": 0,
					"userId": 15585607,
					"media":[
						{
							"id": 0,
							"canView": true,
							"files": {
								"source": {
									"url": "https://raw.githubusercontent.com/allenbenz/winrt-notification/main/resources/test/chick.jpeg"
								},
								"preview": {
									"url": "https://raw.githubusercontent.com/allenbenz/winrt-notification/main/resources/test/flower.jpeg"
								}
							},
							"type": "photo"
						}
					]
				}
			]
		}"#;

		let msg = serde_json::from_str::<message_types::MessageType>(&incoming).unwrap();
		assert!(matches!(
			msg,
			message_types::MessageType::Tagged(message_types::TaggedMessageType::Stories(_))
		));
		msg.handle_message().await.unwrap();
	}
}
