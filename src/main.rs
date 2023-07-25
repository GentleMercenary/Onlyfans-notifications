#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![feature(result_option_inspect)]
#![feature(let_chains)]

#[macro_use]
extern crate log;
extern crate simplelog;

use of_notifier::{self, init, SETTINGS, MANAGER, websocket_client, settings::Settings, get_auth_params};

use chrono::Local;
use anyhow::anyhow;
use futures_util::TryFutureExt;
use image::io::Reader as ImageReader;
use of_client::{client::OFClient, user};
use winrt_toast::{Toast, ToastDuration};
use std::{fs::{self, File}, path::Path};
use tokio::{sync::{Mutex, watch::{channel, Receiver}}, select};
use simplelog::{Config, LevelFilter, WriteLogger, TermLogger,TerminalMode, ColorChoice};
use tokio_tungstenite::tungstenite::error::{Error as ws_error, ProtocolError as protocol_error};
use tao::{event_loop::{EventLoop, ControlFlow, EventLoopProxy}, window::Icon, system_tray::SystemTrayBuilder, menu::{ContextMenu, MenuItemAttributes}, event::{Event, TrayEvent}};

fn get_settings() -> anyhow::Result<Settings> {
	fs::read_to_string("settings.json")
	.inspect_err(|err| error!("Error reading settings.json: {}", err))
	.and_then(|s| serde_json::from_str::<Settings>(&s).map_err(Into::into))
	.inspect_err(|err| error!("Error parsing settings: {}", err))
	.map_err(Into::into)
}

pub async fn make_connection(channel: EventLoopProxy<Events>, mut state: Receiver<State>) -> anyhow::Result<()> {
	info!("Reading authentication parameters");
	let params = get_auth_params()?;

	OFClient::new().authorize(params)
	.and_then(|client| async move {
		info!("Fetching user data");
		let _ = client.get("https://onlyfans.com/api2/v2/init").await?;
		let me = client.get("https://onlyfans.com/api2/v2/users/me")
			.and_then(|response| response.json::<user::Me>().map_err(Into::into))
			.await?;
		
		debug!("{:?}", me);
		let (socket, res) = loop {
			info!("Connecting as {}", me.name);
			let mut socket = websocket_client::WebSocketClient::new()
			.connect(&me.ws_auth_token, &client).await?;
		
			channel.send_event(Events::Connected)?;
			if state.wait_for(|state| matches!(state, State::Connected)).await.is_err() {
				break (socket, Err(anyhow!("Channel is closed")));
			}
			
			let res = select! {
				_ = state.wait_for(|state| matches!(state, State::Disconnecting)) => Ok(()),
				res = socket.message_loop(&client) => res,
			};
			
			if SETTINGS.get().unwrap().lock().await.reconnect && let Err(err) = &res {
				error!("{err}");
				if let Some(ws_error::Protocol(protocol_error::ResetWithoutClosingHandshake)) = err.downcast_ref::<ws_error>() {
					continue;
				}
			}
			break (socket, res);
		};

		info!("Terminating websocket");
		socket.close().await?;
		res
	}).await
}

pub async fn daemon(channel: EventLoopProxy<Events>, mut state: Receiver<State>) {
	loop {
		if state.wait_for(|state| matches!(state, State::Connecting)).await.is_err() {
			break;
		}

		let res = make_connection(channel.clone(), state.clone()).await;
		if 
			let Err(err) = &res &&
			SETTINGS.get().unwrap().blocking_lock().reconnect &&
			err.root_cause().is::<websocket_client::TimeoutExpired>() {
				error!("Timeout expired");
				continue;
		}

		channel.send_event(Events::Disconnected(res)).expect("Sent disconnect message");
		if state.wait_for(|state| matches!(state, State::Disconnected)).await.is_err() {
			break;
		}
	}
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum State { Disconnected, Connected, Connecting, Disconnecting }

#[derive(Debug)]
pub enum Events { Connected, Disconnected(anyhow::Result<()>) }

fn main() -> anyhow::Result<()> {
	if cfg!(debug_assertions) {
		TermLogger::init(LevelFilter::Debug, Config::default(), TerminalMode::Mixed, ColorChoice::Auto)?;
	} else {
		let log_folder = Path::new("logs");
		fs::create_dir_all(log_folder).expect("Created log directory");
		let log_path = log_folder
			.join(Local::now().format("%Y%m%d_%H%M%S").to_string())
			.with_extension("log");
		
		WriteLogger::init(LevelFilter::Info, Config::default(), File::create(log_path)?)?;
	}

	init()?;

	SETTINGS.set(Mutex::new(get_settings()?))
	.expect("Settings read properly");

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
					if let Err(err) = reason {
						error!("Unexpected termination: {:?}", err);

						let mut toast = Toast::new();
						toast
						.text1("OF Notifier")
						.text2("An error occurred, disconnecting")
						.duration(ToastDuration::Long);
				
						MANAGER.get().unwrap().show(&toast)
						.inspect_err(|err| error!("{err}"))
						.unwrap();
					}

					tray_icon.set_icon(second_icon.clone());
					info!("Disconnected");
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
					info!("Closing application");
					MANAGER.get().unwrap().clear()
					.inspect_err(|err| error!("{err}"))
					.unwrap();

					state.send_replace(State::Disconnected);

					*control_flow = ControlFlow::Exit;
				} else if menu_id == reload_id {
					info!("Reloading settings");
					match get_settings() {
						Ok(settings) => {
							*SETTINGS.get().unwrap().blocking_lock() = settings;
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