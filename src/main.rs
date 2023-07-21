#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![feature(result_option_inspect)]
#![feature(let_chains)]

mod client;
mod structs;
mod settings;
mod deserializers;

#[macro_use]
extern crate log;
extern crate simplelog;

use crate::structs::user;

use tokio::{select, sync::{Mutex, Notify}};
use tokio_tungstenite::tungstenite::error::Error as ws_error;
use tokio_tungstenite::tungstenite::error::ProtocolError as protocol_error;
use chrono::Local;
mod websocket_client;
use tempdir::TempDir;
use settings::Settings;
use serde::Deserialize;
use futures::TryFutureExt;
use client::{OFClient, AuthParams};
use image::io::Reader as ImageReader;
use cached::once_cell::sync::OnceCell;
use winrt_toast::{Toast, ToastManager, ToastDuration, register};
use std::{fs::{self, File}, path::Path, sync::Arc, io::{Error, ErrorKind}};
use simplelog::{Config, LevelFilter, WriteLogger, TermLogger,TerminalMode, CombinedLogger, ColorChoice};
use tao::{event_loop::{EventLoop, ControlFlow, EventLoopProxy}, window::Icon, system_tray::SystemTrayBuilder, menu::{ContextMenu, MenuItemAttributes}, event::{Event, TrayEvent}};


static MANAGER: OnceCell<ToastManager> = OnceCell::new();
static SETTINGS: OnceCell<Mutex<Settings>> = OnceCell::new();
static TEMPDIR: OnceCell<TempDir> = OnceCell::new();

fn init() -> anyhow::Result<()> {
	let aum_id = "OFNotifier";
	let icon_path = Path::new("icons").join("icon.ico").canonicalize()
		.inspect_err(|err| error!("{err}"))?;

	register(aum_id, "OF notifier", Some(icon_path.as_path()))
	.inspect_err(|err| error!("{err}"))?;
	
	let _ = MANAGER
	.set(ToastManager::new(aum_id))
	.inspect_err(|_| error!("toast manager set"));

	TempDir::new("OF_thumbs")
	.and_then(|dir| TEMPDIR.set(dir).map_err(|_| Error::new(ErrorKind::Other, "OnceCell couldn't set")))
	.inspect_err(|err| error!("{err}"))?;

	Ok(())
}

fn get_settings() -> anyhow::Result<Settings> {
	fs::read_to_string("settings.json")
	.inspect_err(|err| error!("Error reading settings.json: {}", err))
	.and_then(|s| serde_json::from_str::<Settings>(&s).map_err(Into::into))
	.inspect_err(|err| error!("Error parsing settings: {}", err))
	.map_err(Into::into)
}

fn get_auth_params() -> anyhow::Result<AuthParams> {
	#[derive(Deserialize)]
	struct _AuthParams { auth: AuthParams }

	fs::read_to_string("auth.json")
	.inspect_err(|err| error!("Error reading auth file: {err:?}"))
	.and_then(|data| Ok(serde_json::from_str::<_AuthParams>(&data)?))
	.inspect_err(|err| error!("Error reading auth data: {err:?}"))
	.map(|params| params.auth)
	.inspect(|params| debug!("{params:?}"))
	.map_err(Into::into)
}

async fn make_connection(channel: EventLoopProxy<Events>, cancel: Arc<Notify>, start: Arc<Notify>) {
	loop {
		start.notified().await;
		
		info!("Fetching authentication parameters");
		let cloned_channel = channel.clone();
		let cloned_cancel = cancel.clone();
		let res = futures::future::ready(get_auth_params())
		.and_then(|params| OFClient::new().authorize(params))
		.and_then(|client| async move {
			let me = client.get("https://onlyfans.com/api2/v2/users/me")
				.and_then(|response| response.json::<user::Me>().map_err(Into::into))
				.await?;
			
			debug!("{:?}", me);
			let (socket, res) = loop {
				info!("Connecting as {}", me.name);
				let mut socket = websocket_client::WebSocketClient::new()
					.connect(&me.ws_auth_token, &client).await?;
		
				cloned_channel.send_event(Events::Connected)?;
		
				let res = select! {
					_ = cloned_cancel.notified() => Ok(()),
					res = socket.message_loop(&client) => res,
				};

				if SETTINGS.wait().lock().await.reconnect && let Err(err) = &res {
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
		})
		.await;
	
		channel.send_event(Events::Disconnected(res)).expect("Sent disconnect message");
	}
}

#[derive(PartialEq, Eq)]
enum State { Disconnected, Connected, Connecting, Disconnecting }

#[derive(Debug)]
enum Events { Connected, Disconnected(anyhow::Result<()>) }

fn main() -> anyhow::Result<()> {
	let log_folder = Path::new("logs");
	fs::create_dir_all(log_folder).expect("Created log directory");
	let mut log_path = log_folder.join(Local::now().format("%Y%m%d_%H%M%S").to_string());
	log_path.set_extension("log");

	CombinedLogger::init(
		vec![
			WriteLogger::new(if cfg!(debug_assertions) { LevelFilter::Debug } else { LevelFilter::Info }, Config::default(), File::create(log_path).expect("Created log file")),
			TermLogger::new(if cfg!(debug_assertions) { LevelFilter::Debug } else { LevelFilter::Info }, Config::default(), TerminalMode::Mixed, ColorChoice::Auto)
		]
	)?;

	init()?;

	SETTINGS.set(Mutex::new(get_settings()?))
	.expect("Settings read properly");

	let event_loop = EventLoop::<Events>::with_user_event();
	let proxy = event_loop.create_proxy();

	let first_icon = ImageReader::open(Path::new("icons").join("icon.ico"))
	.map_err(<anyhow::Error>::from)
	.and_then(|reader| reader.decode().map_err(<anyhow::Error>::from))
	.and_then(|image| {
		let width = image.width();
		let height = image.height();
		Icon::from_rgba(image.into_bytes(), width, height)
		.map_err(<anyhow::Error>::from)
	})?;

	let second_icon = ImageReader::open(Path::new("icons").join("icon2.ico"))
	.map_err(<anyhow::Error>::from)
	.and_then(|reader| reader.decode().map_err(<anyhow::Error>::from))
	.and_then(|image| {
		let width = image.width();
		let height = image.height();
		Icon::from_rgba(image.into_bytes(), width, height)
		.map_err(<anyhow::Error>::from)
	})?;

	let mut tray_menu = ContextMenu::new();

	let clear_id = tray_menu.add_item(MenuItemAttributes::new("Clear notifications")).id();
	let reload_id = tray_menu.add_item(MenuItemAttributes::new("Reload settings")).id();
	let quit_id = tray_menu.add_item(MenuItemAttributes::new("Quit")).id();

	let mut tray_icon = SystemTrayBuilder::new(second_icon.clone(), Some(tray_menu))
		.with_tooltip("OF notifier")
		.build(&event_loop)?;

	let cancel = Arc::new(Notify::new());
	let start = Arc::new(Notify::new());
	let runtime = tokio::runtime::Builder::new_multi_thread()
	.enable_all()
	.build()
	.unwrap();
	
	info!("Connecting");
	let mut state = State::Connecting;
	runtime.spawn(make_connection(proxy, cancel.clone(), start.clone()));
	start.notify_one();

	event_loop.run(move |event, _, control_flow| {
		*control_flow = ControlFlow::Wait;
		let _ = tray_icon;

		match event {
			Event::UserEvent(e) => match e {
				Events::Connected => {
					tray_icon.set_icon(first_icon.clone());
					state = State::Connected;
					info!("Connected");
				}
				Events::Disconnected(reason) => {
					if let Err(err) = reason {
						if SETTINGS.wait().blocking_lock().reconnect && err.root_cause().is::<websocket_client::TimeoutExpired>() {
							warn!("Timeout expired");
							start.notify_one();
							return;
						}

						error!("Unexpected termination: {:?}", err);
				
						let mut toast = Toast::new();
						toast
						.text1("OF Notifier")
						.text2("An error occurred, disconnecting")
						.duration(ToastDuration::Long);
				
						MANAGER.wait().show(&toast)
						.inspect_err(|err| error!("{err}"))
						.unwrap();
					}

					tray_icon.set_icon(second_icon.clone());
					state = State::Disconnected;
					info!("Disconnected");
				}
			},
			Event::TrayEvent {event, ..} => {
				if event == TrayEvent::LeftClick {
					match state {
						State::Connected => {
							info!("Disconnecting");
							state = State::Disconnecting;
							cancel.notify_one();
						},
						State::Disconnected => {
							info!("Connecting");
							state = State::Connecting;
							start.notify_one();
						},
						_ => ()
					}
				}
			},
			Event::MenuEvent { menu_id, .. } => {
				if menu_id == quit_id {
					info!("Closing application");
					cancel.notify_one();
					MANAGER.wait().clear()
					.inspect_err(|err| error!("{err}"))
					.unwrap();

					*control_flow = ControlFlow::Exit;
				} else if menu_id == clear_id {
					MANAGER.wait().clear()
					.inspect_err(|err| error!("{err}"))
					.unwrap();
				} else if menu_id == reload_id {
					debug!("Reloading settings");
					match get_settings() {
						Ok(settings) => *SETTINGS.wait().blocking_lock() = settings,
						Err(err) => error!("{err}")
					}
				}
			},
			_ => {}
		}
	});
}