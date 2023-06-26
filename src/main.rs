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

use tokio::select;
use chrono::Local;
mod websocket_client;
use tempdir::TempDir;
use settings::Settings;
use serde::Deserialize;
use futures::TryFutureExt;
use client::{OFClient, AuthParams};
use image::io::Reader as ImageReader;
use cached::once_cell::sync::OnceCell;
use tokio_util::sync::CancellationToken;
use winrt_toast::{Toast, ToastManager, ToastDuration, register};
use std::{fs::{self, File}, path::Path, sync::Arc, io::{Error, ErrorKind}};
use simplelog::{Config, LevelFilter, WriteLogger, TermLogger,TerminalMode, CombinedLogger, ColorChoice};
use tao::{event_loop::{EventLoop, ControlFlow, EventLoopProxy}, window::Icon, system_tray::SystemTrayBuilder, menu::{ContextMenu, MenuItemAttributes}, event::{Event, TrayEvent}};


static MANAGER: OnceCell<ToastManager> = OnceCell::new();
static SETTINGS: OnceCell<Settings> = OnceCell::new();
static TEMPDIR: OnceCell<TempDir> = OnceCell::new();

fn init() {
	let aum_id = "OFNotifier";
	let icon_path = Path::new("icons").join("icon.ico").canonicalize().expect("Found icon file");
	register(aum_id, "OF notifier", Some(icon_path.as_path())).expect("Registered application");
	
	MANAGER
	.set(ToastManager::new(aum_id))
	.expect("Global toast manager set");

	TempDir::new("OF_thumbs")
	.and_then(|dir| TEMPDIR.set(dir).map_err(|_| Error::new(ErrorKind::Other, "OnceCell couldn't set")))
	.expect("Temporary thumbnail created succesfully");
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

async fn make_connection(proxy: EventLoopProxy<Events>, cancel_token: Arc<CancellationToken>) {
	info!("Fetching authentication parameters");

	let cloned_proxy = proxy.clone();
	futures::future::ready(get_auth_params())
	.and_then(|params| OFClient::new().authorize(params))
	.and_then(|client| async move {
		info!("Authorization successful");

		let me = client.get("https://onlyfans.com/api2/v2/users/me")
			.and_then(|response| response.json::<user::Me>().map_err(Into::into))
			.await?;
		
		debug!("{:?}", me);
		info!("Connecting as {}", me.name);
		let mut socket = websocket_client::WebSocketClient::new()
			.connect(&me.ws_auth_token, &client).await?;

		cloned_proxy.send_event(Events::Connected)?;

		let res = select! {
			_ = cancel_token.cancelled() => Ok(()),
			res = socket.message_loop(client) => res,
		};

		info!("Terminating websocket");
		socket.close().await?;
		res
	})
	.unwrap_or_else(|err| {
		error!("Unexpected termination: {:?}", err);

		let mut toast = Toast::new();
		toast
		.text1("OF Notifier")
		.text2("An error occurred, disconnecting")
		.duration(ToastDuration::Long);

		MANAGER.wait().show(&toast).expect("Showed error notification");
	})
	.await;

	proxy.send_event(Events::Disconnected).expect("Sent disconnect message")
}

#[derive(PartialEq, Eq)]
enum State { Disconnected, Connected }

#[derive(Clone, Eq, PartialEq, Debug)]
enum Events { Connected, Disconnected, /* Clear,*/ }

#[tokio::main]
async fn main() -> anyhow::Result<()> {
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

	init();

	let s = fs::read_to_string("settings.json")
		.inspect_err(|err| error!("Error reading settings.json: {}", err))?;

	SETTINGS
		.set(
			serde_json::from_str::<Settings>(&s)
				.inspect_err(|err| error!("Error parsing settings: {}", err))?,
		)
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

	let quit_id = tray_menu.add_item(MenuItemAttributes::new("Quit")).id();
	let clear_id = tray_menu.add_item(MenuItemAttributes::new("Clear notifications")).id();

	let mut tray_icon = SystemTrayBuilder::new(second_icon.clone(), Some(tray_menu))
		.with_tooltip("OF notifier")
		.build(&event_loop)?;

	let mut state = State::Disconnected;
	let mut cancel_token = Arc::new(CancellationToken::new());

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
				Events::Disconnected => {
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
							cancel_token.cancel();
						},
						State::Disconnected => {
							cancel_token = Arc::new(CancellationToken::new());
							info!("Connecting");
							tokio::spawn(make_connection(proxy.clone(), cancel_token.clone()));
						}
					}
				}
			},
			Event::MenuEvent { menu_id, .. } => {
				if menu_id == quit_id {
					info!("Closing application");
					cancel_token.cancel();
					MANAGER.wait().clear().expect("Cleared notifications");
					*control_flow = ControlFlow::Exit;
				} else if menu_id == clear_id {
					MANAGER.wait().clear().expect("Cleared notifications");
				}
			},
			_ => {}
		}
	});
}