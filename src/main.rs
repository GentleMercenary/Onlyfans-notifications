#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![feature(let_chains)]

#[macro_use]
extern crate log;
extern crate simplelog;

use futures::FutureExt;
use of_notifier::{init, SETTINGS, MANAGER, websocket_client, settings::Settings, get_auth_params};

use chrono::Local;
use anyhow::anyhow;
use futures_util::TryFutureExt;
use image::io::Reader as ImageReader;
use of_client::{client::OFClient, user};
use winrt_toast::{Toast, ToastDuration};
use std::{fs::{self, File}, path::Path};
use tokio::sync::{watch::{channel, Receiver}, RwLock};
use simplelog::{ColorChoice, CombinedLogger, ConfigBuilder, TermLogger, TerminalMode, WriteLogger};
use tokio_tungstenite::tungstenite::error::{Error as ws_error, ProtocolError as protocol_error};
use tao::{event_loop::{EventLoop, ControlFlow, EventLoopProxy}, window::Icon, system_tray::SystemTrayBuilder, menu::{ContextMenu, MenuItemAttributes}, event::{Event, TrayEvent}};

fn get_settings() -> anyhow::Result<Settings> {
	fs::read_to_string("settings.json")
	.inspect_err(|err| error!("Error reading settings.json: {}", err))
	.and_then(|s| serde_json::from_str::<Settings>(&s).map_err(Into::into))
	.inspect_err(|err| error!("Error parsing settings: {}", err))
	.map_err(Into::into)
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum State { Disconnected, Connected, Connecting, Disconnecting }

pub async fn make_connection(channel: EventLoopProxy<Events>, mut state: Receiver<State>) -> anyhow::Result<()> {
	info!("Reading authentication parameters");
	let client = OFClient::new(get_auth_params()?).await?;
	
	info!("Fetching user data");
	let me = client.get("https://onlyfans.com/api2/v2/users/me")
		.and_then(|response| response.json::<user::Me>())
		.await?;
	
	debug!("{:?}", me);
	info!("Connecting as {}", me.name);
	let mut socket = websocket_client::WebSocketClient::new()
	.connect(&me.ws_url, &me.ws_auth_token).await?;

	channel.send_event(Events::Connected)?;
	let res = {
		if state.wait_for(|state| matches!(state, State::Connected)).await.is_err() {
			Err(anyhow!("Channel is closed"))
		} else {
			let cancel = state.wait_for(|state| matches!(state, State::Disconnecting)).map(|_| ());
			socket.message_loop(&client, cancel).await
		}
	};
	
	info!("Terminating websocket");
	socket.close().await?;
	res
}

pub async fn daemon(channel: EventLoopProxy<Events>, mut state: Receiver<State>) {
	loop {
		if state.wait_for(|state| matches!(state, State::Connecting)).await.is_err() {
			break;
		}

		let res = loop {
			let res = make_connection(channel.clone(), state.clone()).await;
			if let Err(err) = &res && SETTINGS.get().unwrap().read().await.reconnect {
				error!("{err}");
				if let Some(ws_error::Protocol(protocol_error::ResetWithoutClosingHandshake)) = err.downcast_ref::<ws_error>() {
					continue;
				}

				if err.root_cause().is::<websocket_client::TimeoutExpired>() {
					continue;
				}
			}

			break res;
		};

		channel.send_event(Events::Disconnected(res)).expect("Sent disconnect message");
		if state.wait_for(|state| matches!(state, State::Disconnected)).await.is_err() {
			break;
		}
	}
}

#[derive(Debug)]
pub enum Events { Connected, Disconnected(anyhow::Result<()>) }

fn main() -> anyhow::Result<()> {
	SETTINGS.set(RwLock::new(get_settings()?))
	.expect("Settings read properly");

	let log_path = Path::new("logs")
		.join(Local::now().format("%Y%m%d_%H%M%S").to_string())
		.with_extension("log");

	log_path.parent()
	.and_then(|dir| fs::create_dir_all(dir).ok())
	.expect("Created log directory");
	
	let log_config = ConfigBuilder::default()
		.add_filter_ignore_str("reqwest::connect")
		.add_filter_ignore_str("cookie_store::cookie_store")
		.add_filter_ignore_str("strip_markdown")
		.build();

	let log_level = SETTINGS.get().unwrap().blocking_read().log_level;
	CombinedLogger::init(vec![
		TermLogger::new(log_level, log_config.clone(), TerminalMode::Mixed, ColorChoice::Auto),
		WriteLogger::new(log_level, log_config, File::create(log_path)?)
	])?;

	init()?;

	let event_loop = EventLoop::<Events>::with_user_event();

	let read_image = |path: &Path| {
		ImageReader::open(path)
		.map_err(<anyhow::Error>::from)
		.and_then(|reader| reader.decode().map_err(<anyhow::Error>::from))
		.and_then(|image| {
			let width = image.width();
			let height = image.height();
			Icon::from_rgba(image.into_bytes(), width, height)
			.map_err(<anyhow::Error>::from)
		})
	};

	let first_icon = read_image(&Path::new("icons").join("icon.ico"))?;
	let second_icon = read_image(&Path::new("icons").join("icon2.ico"))?;

	let mut tray_menu = ContextMenu::new();

	let reload_id = tray_menu.add_item(MenuItemAttributes::new("Reload settings")).id();
	let quit_id = tray_menu.add_item(MenuItemAttributes::new("Quit")).id();

	let mut tray_icon = SystemTrayBuilder::new(second_icon.clone(), Some(tray_menu))
		.with_tooltip("OF notifier")
		.build(&event_loop)?;

	let (state, rx) = channel(State::Disconnected);
	let runtime = tokio::runtime::Builder::new_multi_thread()
	.enable_all()
	.build()
	.unwrap();
	
	info!("Connecting");
	runtime.spawn(daemon(event_loop.create_proxy(), rx));
	state.send_replace(State::Connecting);

	event_loop.run(move |event, _, control_flow| {
		*control_flow = ControlFlow::Wait;
		let _ = tray_icon;

		match event {
			Event::UserEvent(e) => match e {
				Events::Connected => {
					state.send_replace(State::Connected);
					tray_icon.set_icon(first_icon.clone());
					info!("Connected");
				}
				Events::Disconnected(reason) => {
					state.send_replace(State::Disconnected);
					tray_icon.set_icon(second_icon.clone());
					info!("Disconnected");

					if reason.is_err() {
						let mut toast = Toast::new();
						toast
						.text1("OF Notifier")
						.text2("An error occurred, disconnecting")
						.duration(ToastDuration::Long);
				
						MANAGER.get().unwrap().show(&toast)
						.inspect_err(|err| error!("{err}"))
						.unwrap();
					}

				}
			},
			Event::TrayEvent {event, ..} => {
				if event == TrayEvent::LeftClick {
					let _state = state.borrow().clone();
					match _state {
						State::Connected => {
							state.send_replace(State::Disconnecting);
							info!("Disconnecting");
						},
						State::Disconnected => {
							state.send_replace(State::Connecting);
							info!("Connecting");
						},
						_ => ()
					}
				}
			},
			Event::MenuEvent { menu_id, .. } => {
				if menu_id == quit_id {
					state.send_replace(State::Disconnecting);
					info!("Disconnecting");
					
					state.send_replace(State::Disconnected);
					info!("Closing application");

					*control_flow = ControlFlow::Exit;
				} else if menu_id == reload_id {
					info!("Reloading settings");
					match get_settings() {
						Ok(settings) => {
							*SETTINGS.get().unwrap().blocking_write() = settings;
							info!("Successfully updated settings")
						},
						Err(err) => error!("{err}")
					}
				}
			},
			_ => {}
		}
	});
}