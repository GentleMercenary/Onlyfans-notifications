#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use log::*;
use of_client::SharedRequestHeaders;
use of_notifier::{get_auth_params, handlers::{Context, Handler}, helpers::show_notification, init_cdm, init_client, settings::Settings, FileParseError};
use of_daemon::{socket::SocketError, tungstenite::error::{Error as WSError, ProtocolError}, Daemon, DaemonError};
use tray_icon::{menu::{Menu, MenuEvent, MenuItem}, Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use winit::{application::ApplicationHandler, event, event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy}, window::WindowId};
use winrt_toast::{Toast, ToastDuration};
use std::{fs::{self, File}, path::Path, sync::{Arc, RwLock}};
use simplelog::{ColorChoice, CombinedLogger, ConfigBuilder, TermLogger, TerminalMode, WriteLogger};
use chrono::Local;
use tokio::sync::Notify;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
	let settings = get_settings()
		.expect("Reading settings");

	let log_folder = Path::new("logs");
	fs::create_dir_all(log_folder)
	.expect("Creating log directory");
	
	let log_config = ConfigBuilder::default()
		.add_filter_ignore_str("reqwest::connect")
		.add_filter_ignore_str("cookie_store::cookie_store")
		.add_filter_ignore_str("tokio_tungstenite")
		.add_filter_ignore_str("tungstenite")
		.build();

	let log_path = log_folder.join(Local::now().format("%Y%m%d_%H%M%S").to_string()).with_extension("log");
	let log_level = settings.log_level;
	CombinedLogger::init(vec![
		TermLogger::new(log_level, log_config.clone(), TerminalMode::Mixed, ColorChoice::Auto),
		WriteLogger::new(log_level, log_config, File::create(log_path)?)
	])?;

	let client = init_client()?;
	let client_params = client.headers.clone();

	let cdm = init_cdm()
		.inspect_err(|e| warn!("CDM could not be initialized: {e}"))
		.ok();

	let settings = Arc::new(RwLock::new(settings));

	let event_loop = EventLoop::<Events>::with_user_event()
		.build()
		.unwrap();

	let (toggle_daemon, _) = Daemon::new()
		.on_start({
			let proxy = event_loop.create_proxy();
			move || { let _ = proxy.send_event(Events::Connected); }
		})
		.on_disconnect({
			let proxy = event_loop.create_proxy();
			move |e| { let _ = proxy.send_event(Events::Disconnected(e)); }
		})
		.on_message({
			let context = Context::new(client.clone(), cdm, settings.clone()).unwrap();
			move |message| { let _ = message.handle(&context); }
		})
		.build(client);

	let mut app = App {
		should_quit: false,
		state: AppState::Disconnected,
		tray: None,
		event_loop: event_loop.create_proxy(),
		settings,
		client_params,
		toggle_daemon,
	};

	event_loop.run_app(&mut app).unwrap();
	Ok(())
}

enum Events {
	Connected,
	Disconnected(Result<(), DaemonError>),
	TrayEvent(TrayIconEvent),
	MenuEvent(MenuEvent),
}

#[derive(Debug, PartialEq)]
enum AppState { Connected, Connecting, Disconnected, Disconnecting }

struct MenuItems {
	quit: MenuItem,
	reload_settings: MenuItem,
	reload_auth: MenuItem,
}

struct Icons {
	connected: Icon,
	disconnected: Icon,
}

struct Tray {
	tray: TrayIcon,
	menu_items: MenuItems,
	icons: Icons,
}

struct App {
	should_quit: bool,
	state: AppState,
	tray: Option<Tray>,
	event_loop: EventLoopProxy<Events>,
	settings: Arc<RwLock<Settings>>,
	client_params: Arc<SharedRequestHeaders>,
	toggle_daemon: Arc<Notify>,
}

impl App {
	fn init_connection(&mut self) {
		info!("Connecting");
		self.state = AppState::Connecting;
		self.toggle_daemon.notify_one();
	}

	fn close_connection(&mut self) {
		info!("Closing connection");
		self.state = AppState::Disconnecting;
		self.toggle_daemon.notify_one();
	}
}

macro_rules! exit {
	($event_loop: ident) => {{
		info!("Closing application");
		$event_loop.exit();
		return;
	}};
}

impl ApplicationHandler<Events> for App {
	fn resumed(&mut self, _event_loop: &ActiveEventLoop) {}
	fn window_event(&mut self, _event_loop: &ActiveEventLoop, _window_id: WindowId, _event: event::WindowEvent) {}

	fn new_events(&mut self, _event_loop: &ActiveEventLoop, cause: event::StartCause) {
		if cause == event::StartCause::Init {
			let connected_icon = Icon::from_path(Path::new("icons").join("icon.ico"), None)
				.inspect_err(|e| error!("Failed to create connected icon: {e}"))
				.unwrap();
		
			let disconnected_icon = Icon::from_path(Path::new("icons").join("icon2.ico"), None)
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
			]).unwrap();
		
			{
				let event_loop = self.event_loop.clone();
				MenuEvent::set_event_handler(Some(move |event| {
					let _ = event_loop.send_event(Events::MenuEvent(event));
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
				let event_loop = self.event_loop.clone();
				TrayIconEvent::set_event_handler(Some(move |event| {
					let _ = event_loop.send_event(Events::TrayEvent(event));
				}));
			}

			self.tray = Some(Tray {
				tray,
				menu_items: MenuItems {
					reload_settings: reload_settings_item,
					quit: quit_item,
					reload_auth: reload_auth_item
				},
				icons: Icons {
					connected: connected_icon,
					disconnected: disconnected_icon
				}
			});

			self.init_connection();
		}
	}

	fn user_event(&mut self, event_loop: &ActiveEventLoop, event: Events) {
		match event {
			Events::Connected => {
				if let Some(Tray {tray, icons: Icons { connected, .. }, ..}) = &self.tray {
					tray.set_icon(Some(connected.clone())).unwrap();
				}
	
				info!("Connected");
				self.state = AppState::Connected;
			}
			Events::Disconnected(result) => {
				if let Some(Tray {tray, icons: Icons { disconnected, .. }, ..}) = &self.tray {
					tray.set_icon(Some(disconnected.clone())).unwrap();
				}

				info!("Disconnected");
				self.state = AppState::Disconnected;

				if self.should_quit { exit!(event_loop); }

				if let Err(err) = result {
					if self.settings.read().unwrap().reconnect {
						if let DaemonError::Socket(
								SocketError::TimeoutExpired |
								SocketError::Socket(WSError::Protocol(ProtocolError::ResetWithoutClosingHandshake))
							) = err
						{
							info!("Attempting to reconnect");
							self.init_connection();
							return;
						}
					}
	
					let mut toast = Toast::new();
					toast
					.text1("OF Notifier")
					.text2("An error occurred")
					.duration(ToastDuration::Long);
	
					let _ = show_notification(&toast);
				} 
			},
			Events::MenuEvent(MenuEvent { id }) => {
				let menu_items = &self.tray.as_ref().unwrap().menu_items;

				if id == menu_items.quit.id() {
					self.should_quit = true;
					match self.state {
						AppState::Connected | AppState::Connecting => self.close_connection(),
						AppState::Disconnected => exit!(event_loop),
						AppState::Disconnecting => ()
					}
				} else if id == menu_items.reload_settings.id() {
					info!("Reloading settings");
					if let Ok(new_settings) = get_settings() {
						*self.settings.write().unwrap() = new_settings;
						info!("Successfully updated settings");
					}
				} else if id == menu_items.reload_auth.id() {
					info!("Reloading authentication parameters");
					if let Ok(new_auth) = get_auth_params() {
						let mut params_lock = self.client_params.write().unwrap();
						params_lock.x_bc = new_auth.x_bc;
						params_lock.user_id = new_auth.user_id;
						params_lock.user_agent = new_auth.user_agent;
						*params_lock.cookie.write().unwrap() = new_auth.cookie;

						info!("Successfully updated authentication parameters");
					}
				}
			},
			Events::TrayEvent(tray_event) => {
				 if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Down, .. } = tray_event {
					match self.state {
						AppState::Disconnected => self.init_connection(),
						AppState::Connected | AppState::Connecting => self.close_connection(),
						AppState::Disconnecting => ()
					}
				}
			},
		}
	}
}

fn get_settings() -> Result<Settings, FileParseError> {
	let data = fs::read_to_string("settings.json")
	.inspect_err(|err| error!("Error reading settings: {err}"))?;

	serde_json::from_str::<Settings>(&data).map_err(Into::into)
	.inspect_err(|err| error!("Error parsing settings: {err}"))
}