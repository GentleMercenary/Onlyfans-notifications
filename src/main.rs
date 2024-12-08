#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[macro_use]
extern crate log;
extern crate simplelog;

use of_notifier::{get_auth_params, handlers::handle_message, helpers::{init_client, show_notification}, settings::Settings, socket::{SocketError, WebSocketClient}, structs::Message, FileParseError};
use rand::{rngs::StdRng, Rng, SeedableRng};
use rand_distr::{Distribution, Exp1, Standard};
use chrono::{Local, Utc};
use futures_util::TryFutureExt;
use of_client::{client::OFClient, user};
use serde::Serialize;
use tray_icon::{menu::{Menu, MenuEvent, MenuItem}, Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use winit::{application::ApplicationHandler, event_loop::{ControlFlow, EventLoop, EventLoopProxy}};
use winrt_toast::{Toast, ToastDuration};
use std::{fs::{self, File}, path::Path, sync::Arc, time::Duration};
use tokio::{sync::RwLock, task::JoinHandle, time::sleep};
use simplelog::{ColorChoice, CombinedLogger, ConfigBuilder, TermLogger, TerminalMode, WriteLogger};
use tokio_tungstenite::tungstenite::error::{Error as ws_error, ProtocolError as protocol_error};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
	let settings = get_settings()
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
		.add_filter_ignore_str("tokio_tungstenite")
		.add_filter_ignore_str("tungstenite")
		.build();

	let log_level = settings.log_level;
	CombinedLogger::init(vec![
		TermLogger::new(log_level, log_config.clone(), TerminalMode::Mixed, ColorChoice::Auto),
		WriteLogger::new(log_level, log_config, File::create(log_path)?)
	])?;

	let event_loop = EventLoop::<Events>::with_user_event()
	.build()
	.unwrap();

	event_loop.set_control_flow(ControlFlow::Wait);

	let connected_icon = tray_icon::Icon::from_path(
		Path::new("icons").join("icon.ico"),
		None
	).expect("Failed to create connected icon (icon.ico)");

	let disconnected_icon = tray_icon::Icon::from_path(
		Path::new("icons").join("icon2.ico"),
		None
	).expect("Failed to create disconnected icon (icon2.ico)");

	let tray_menu = Menu::new();
	let reload_settings_item = MenuItem::new("Reload settings", true, None);
	let reload_auth_item = MenuItem::new("Reload auth", true, None);
	let quit_item = MenuItem::new("Quit", true, None);
	tray_menu.append_items(&[
		&reload_auth_item,
		&reload_settings_item,
		&quit_item,
	])?;

	let proxy = event_loop.create_proxy();
	MenuEvent::set_event_handler(Some(move |event| {
		proxy.send_event(Events::MenuEvent(event)).unwrap();
	}));

	let tray = TrayIconBuilder::new()
	.with_tooltip("OF Notifier")
	.with_icon(disconnected_icon.clone())
	.with_menu(Box::new(tray_menu))
	.with_menu_on_left_click(false)
	.build()
	.unwrap();

	let proxy = event_loop.create_proxy();
	TrayIconEvent::set_event_handler(Some(move |event| {
		proxy.send_event(Events::TrayEvent(event)).unwrap();
	}));

	let client = init_client()?;

	let mut app = App {
		tray,
		proxy: event_loop.create_proxy(),
		connected_icon,
		disconnected_icon,
		settings: Arc::new(RwLock::new(settings)),
		state: AppState::Connecting,
		client,
		connection: None,
		menu_items: MenuItems {
			quit: quit_item,
			reload_settings: reload_settings_item,
			reload_auth: reload_auth_item,
		},
	};

	event_loop.run_app(&mut app).unwrap();
	Ok(())
}

#[derive(Debug, Serialize)]
enum Pages {
	Collections,
	Subscribes,
	Profile,
	Chats,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ClickStats {
	page: Pages,
	block: &'static str,
	event_time: String
}

impl Distribution<ClickStats> for Standard {
	fn sample<R: rand::Rng + ?Sized>(&self, rng: &mut R) -> ClickStats {
		ClickStats {
			page: match rng.gen_range(0..=3) {
				0 => Pages::Collections,
				1 => Pages::Subscribes,
				2 => Pages::Profile,
				_ => Pages::Chats
			},
			block: "Menu",
			event_time: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
		}
	}
}

#[derive(Debug)]
enum Events {
	Connected,
	Disconnected(anyhow::Error),
	TrayEvent(TrayIconEvent),
	MenuEvent(MenuEvent),
	MessageReceived(Message),
	Reconnect
}

macro_rules! propagate_proxy {
	($proxy:expr, $expr:expr) => {
		match $expr {
			Ok(val) => val,
			Err(e) => {
				$proxy.send_event(Events::Disconnected(e.into())).unwrap();
				return;
			}
		}
	};
}

#[derive(PartialEq)]
enum AppState { Connected, Connecting, Disconnected }

struct MenuItems {
	quit: MenuItem,
	reload_settings: MenuItem,
	reload_auth: MenuItem,
}

struct Connection {
	stream_handle: JoinHandle<()>,
	activity_handle: JoinHandle<()>
}

impl Drop for Connection {
	fn drop(&mut self) {
		self.stream_handle.abort();
		self.activity_handle.abort();
	}
}

struct App {
	tray: TrayIcon,
	proxy: EventLoopProxy<Events>,
	connected_icon: Icon,
	disconnected_icon: Icon,
	settings: Arc<RwLock<Settings>>,
	client: OFClient,
	state: AppState,
	connection: Option<Connection>,
	menu_items: MenuItems
}

impl App {
	fn init_connection(&mut self) {
		self.state = AppState::Connecting;
		info!("Connecting");

		let activity_handle = tokio::spawn({
			let client = self.client.clone();
			async move {
				let rng = StdRng::from_entropy();
				let mut intervals = rng.sample_iter(Exp1).map(|v: f32| Duration::from_secs_f32(v * 60.0));
				
				loop {
					sleep(intervals.next().unwrap()).await;
					let click = rand::random::<ClickStats>();
					trace!("Simulating site activity: {}", serde_json::to_string(&click).unwrap());
					let _ = client.post("https://onlyfans.com/api2/v2/users/clicks-stats", Some(&click)).await;
				}
			}
		});
		
		let stream_handle = tokio::spawn({
			let client = self.client.clone();
			let proxy = self.proxy.clone();
			async move {
				info!("Fetching user data");
				let me = propagate_proxy!(proxy, 
					client.get("https://onlyfans.com/api2/v2/users/me")
					.and_then(|response| response.json::<user::Me>())
					.await
				);
				
				debug!("{me:?}");
				info!("Connecting as {}", me.name);
				let (_socket, mut stream) = propagate_proxy!(proxy,
					WebSocketClient::new()
					.connect(&me.ws_url, &me.ws_auth_token)
					.inspect_err(|err| error!("Error connecting: {err}"))
					.await
				);
			
				proxy.send_event(Events::Connected).unwrap();

				loop {
					if let Some(message) = stream.recv().await {
						match message {
							Ok(msg) => proxy.send_event(Events::MessageReceived(msg)).unwrap(),
							Err(e) => {
								info!("Terminating websocket");
								proxy.send_event(Events::Disconnected(e.into())).unwrap();
								break;
							}
						}
					}
				};
			}
		});

		self.connection = Some(Connection {
			activity_handle,
			stream_handle
		});
	}

	fn close_connection(&mut self) {
		if self.connection.is_none() { return; }
		
		self.connection = None;
		self.tray.set_icon(Some(self.disconnected_icon.clone())).unwrap();
		self.state = AppState::Disconnected;
		info!("Disconnected");
	}
}

impl ApplicationHandler<Events> for App {
	fn resumed(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop) {}
	fn window_event(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop, _window_id: winit::window::WindowId, _event: winit::event::WindowEvent) {}

	fn new_events(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop, cause: winit::event::StartCause) {
		if cause == winit::event::StartCause::Init {
			self.init_connection();
		}
	}

	fn user_event(&mut self, event_loop: &winit::event_loop::ActiveEventLoop, event: Events) {
		match event {
			Events::Connected => {
				self.tray.set_icon(Some(self.connected_icon.clone())).unwrap();
				self.state = AppState::Connected;
				info!("Connected");
			},
			Events::Disconnected(err) => {
				self.close_connection();

				tokio::spawn({
					let settings = self.settings.clone();
					let proxy = self.proxy.clone();
					async move {
						if settings.read().await.reconnect {
							if let Ok(SocketError::TimeoutExpired | SocketError::SocketError(ws_error::Protocol(protocol_error::ResetWithoutClosingHandshake))) = err.downcast::<SocketError>() {
								proxy.send_event(Events::Reconnect).unwrap();
								return;
							}
						}
		
						let mut toast = Toast::new();
						toast
						.text1("OF Notifier")
						.text2("An error occurred, disconnecting")
						.duration(ToastDuration::Long);
		
						let _ = show_notification(&toast);
					}
				});
			},
			Events::Reconnect => { self.init_connection(); },
			Events::TrayEvent(TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Down, .. }) => {
				if self.state == AppState::Connected { self.close_connection(); } 
				else if self.state == AppState::Disconnected { self.init_connection(); }
			},
			Events::MenuEvent(MenuEvent { id }) => {
				if id == self.menu_items.quit.id() {
					self.close_connection();
					info!("Closing application");

					event_loop.exit();
				} else if id == self.menu_items.reload_settings.id() {
					info!("Reloading settings");
					if let Ok(new_settings) = get_settings() {
						tokio::spawn({
							let settings = self.settings.clone();
							async move {
								*settings.write().await = new_settings;
								info!("Successfully updated settings");
							}
						});
					}
				} else if id == self.menu_items.reload_auth.id() {
					info!("Reloading authentication parameters");
					if let Ok(new_auth) = get_auth_params() {
						self.client.set_auth_params(new_auth);
						info!("Successfully updated authentication parameters");
					}
				}
			},
			Events::MessageReceived(msg) => {
				let client = self.client.clone();
				let settings = self.settings.clone();
				tokio::spawn(async move {
					let _ = handle_message(msg, &client, settings.as_ref()).await;
				});
			}
			_ => ()
		}
	}
}

fn get_settings() -> Result<Settings, FileParseError> {
	fs::read_to_string("settings.json")
	.inspect_err(|err| error!("Error reading settings: {err}"))
	.and_then(|s| serde_json::from_str::<Settings>(&s).map_err(Into::into))
	.inspect_err(|err| error!("Error parsing settings: {err}"))
	.map_err(Into::into)
}