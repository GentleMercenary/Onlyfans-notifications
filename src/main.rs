#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[macro_use]
extern crate log;
extern crate simplelog;

use of_notifier::{get_auth_params, handlers::handle_message, helpers::{init_client, show_notification}, settings::Settings, FileParseError};
use chrono::Local;
use of_socket::{socket::SocketError, structs::Message, DaemonError, ProtocolError, SocketDaemon, WSError};
use of_client::OFClient;
use tray_icon::{menu::{Menu, MenuEvent, MenuItem}, Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use winit::{application::ApplicationHandler, event_loop::{ControlFlow, EventLoop, EventLoopProxy}};
use winrt_toast::{Toast, ToastDuration};
use std::{fs::{self, File}, path::Path, sync::Arc};
use tokio::{runtime::Handle, sync::RwLock, task};
use simplelog::{ColorChoice, CombinedLogger, ConfigBuilder, TermLogger, TerminalMode, WriteLogger};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
	let settings = get_settings()
		.expect("Reading settings");

	let log_path = Path::new("logs")
		.join(Local::now().format("%Y%m%d_%H%M%S").to_string())
		.with_extension("log");

	log_path.parent()
	.and_then(|dir| fs::create_dir_all(dir).ok())
	.expect("Creating log directory");
	
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
	let proxy = event_loop.create_proxy();

	let connected_icon = tray_icon::Icon::from_path(Path::new("icons").join("icon.ico"), None)
	.inspect_err(|e| error!("Failed to create connected icon: {e}"))
	.unwrap();

	let disconnected_icon = tray_icon::Icon::from_path(Path::new("icons").join("icon2.ico"), None)
	.inspect_err(|e| error!("Failed to create disconnected icon: {e}"))
	.unwrap();

	let tray_menu = Menu::new();
	let reload_settings_item = MenuItem::new("Reload settings", true, None);
	let reload_auth_item = MenuItem::new("Reload auth", true, None);
	let quit_item = MenuItem::new("Quit", true, None);
	tray_menu.append_items(&[
		&reload_auth_item,
		&reload_settings_item,
		&quit_item,
	])?;

	{
		let proxy = proxy.clone();
		MenuEvent::set_event_handler(Some(move |event| {
			proxy.send_event(Events::MenuEvent(event)).unwrap();
		}));
	}

	let tray = TrayIconBuilder::new()
	.with_tooltip("OF Notifier")
	.with_icon(disconnected_icon.clone())
	.with_menu(Box::new(tray_menu))
	.with_menu_on_left_click(false)
	.build()
	.unwrap();

	{
		let proxy = proxy.clone();
		TrayIconEvent::set_event_handler(Some(move |event| {
			proxy.send_event(Events::TrayEvent(event)).unwrap();
		}));
	}

	let client = init_client()?;
	let daemon = SocketDaemon::new()
		.on_disconnect({
			let proxy = proxy.clone();
			move |e| { proxy.send_event(Events::Disconnected(e)).unwrap(); }
		})
		.on_message({
			let proxy = proxy.clone();
			move |msg| { proxy.send_event(Events::MessageReceived(msg)).unwrap(); }
		});

	let mut app = App {
		tray,
		proxy,
		connected_icon,
		disconnected_icon,
		settings: Arc::new(RwLock::new(settings)),
		state: AppState::Connecting,
		client,
		daemon,
		menu_items: MenuItems {
			quit: quit_item,
			reload_settings: reload_settings_item,
			reload_auth: reload_auth_item,
		},
	};

	event_loop.run_app(&mut app).unwrap();
	Ok(())
}

#[derive(Debug)]
enum Events {
	Connected,
	Disconnected(DaemonError),
	TrayEvent(TrayIconEvent),
	MenuEvent(MenuEvent),
	MessageReceived(Message),
	Reconnect
}

#[derive(PartialEq)]
enum AppState { Connected, Connecting, Disconnected }

struct MenuItems {
	quit: MenuItem,
	reload_settings: MenuItem,
	reload_auth: MenuItem,
}

struct App {
	tray: TrayIcon,
	proxy: EventLoopProxy<Events>,
	connected_icon: Icon,
	disconnected_icon: Icon,
	settings: Arc<RwLock<Settings>>,
	client: OFClient,
	state: AppState,
	daemon: SocketDaemon,
	menu_items: MenuItems
}

impl App {
	fn init_connection(&mut self) {
		self.state = AppState::Connecting;
		task::block_in_place(|| {
			let client = self.client.clone();
			Handle::current().block_on(async move {
				match self.daemon.start(client).await {
					Ok(_) => self.proxy.send_event(Events::Connected).unwrap(),
					Err(err) => self.proxy.send_event(Events::Disconnected(err)).unwrap()
				}
			});
		});
	}

	fn close_connection(&mut self) {
		self.daemon.stop();
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
			info!("Connecting");
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
				if self.state == AppState::Disconnected { return; }
				self.close_connection();

				tokio::spawn({
					let settings = self.settings.clone();
					let proxy = self.proxy.clone();
					async move {
						if settings.read().await.reconnect {
							if let DaemonError::Socket(
									SocketError::TimeoutExpired |
									SocketError::SocketError(WSError::Protocol(ProtocolError::ResetWithoutClosingHandshake))
								) = err
							{
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
			Events::Reconnect => {
				info!("Reconnecting");
				self.init_connection();
			},
			Events::TrayEvent(TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Down, .. }) => {
				if self.state == AppState::Connected { self.close_connection(); } 
				else if self.state == AppState::Disconnected {
					info!("Connecting");
					self.init_connection();
				}
			},
			Events::MenuEvent(MenuEvent { id }) => {
				if id == self.menu_items.quit.id() {
					if self.state != AppState::Disconnected { self.close_connection(); }
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