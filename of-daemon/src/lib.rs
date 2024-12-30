#[macro_use]
extern crate log;

pub mod structs;
pub mod socket;

pub mod tungstenite { pub use tokio_tungstenite::tungstenite::error; }

use std::{sync::Arc, time::Duration};
use chrono::Utc;
use futures::{StreamExt, TryFutureExt};
use of_client::{OFClient, reqwest, user};
use rand::{rngs::StdRng, Rng, SeedableRng};
use rand_distr::{Distribution, Exp1, Standard};
use serde::Serialize;
use socket::Connected;
use thiserror::Error;
use tokio::{pin, sync::Notify, task::JoinHandle, time::sleep};
use crate::{socket::{SocketError, WebSocketClient}, structs::Message};

#[derive(Error, Debug)]
pub enum DaemonError {
	#[error("{0}")]
	Socket(#[from] SocketError),
	#[error("{0}")]
	Request(#[from] reqwest::Error)
}

pub struct Daemon {
	started_callback: Option<Box<dyn Fn() + Send>>,
	message_callback: Option<Box<dyn Fn(Message) + Send>>,
	disconnect_callback: Option<Box<dyn Fn(Result<(), DaemonError>) + Send>>,
}

impl Daemon {
	pub fn new() -> Self {
		Self {
			started_callback: None,
			message_callback: None,
			disconnect_callback: None
		}
	}

	pub fn on_start(mut self, f: impl Fn() + Send + 'static) -> Self {
		self.started_callback = Some(Box::new(f));
		self
	}

	pub fn on_message(mut self, f: impl Fn(Message) + Send + 'static) -> Self {
		self.message_callback = Some(Box::new(f));
		self
	}

	pub fn on_disconnect(mut self, f: impl Fn(Result<(), DaemonError>) + Send + 'static) -> Self {
		self.disconnect_callback = Some(Box::new(f));
		self
	}

	pub fn build(self, client: OFClient) -> (Arc<Notify>, JoinHandle<()>) {
		let notify = Arc::new(Notify::new());

		let handle = tokio::spawn({
			let notify = notify.clone();
			async move {
				loop {
					notify.notified().await;

					let mut socket = tokio::select! {
						_ = notify.notified() => {
							if let Some(ref callback) = self.disconnect_callback { callback(Ok(())) }
							continue;
						},
						val = connect(&client) => match val {
							Ok(val) => val,
							Err(err) => {
								if let Some(ref callback) = self.disconnect_callback { callback(Err(err)) }
								continue;
							}
						}
					};
	
					if let Some(ref callback) = self.started_callback { callback(); }
	
					let activity = simulate_activity(&client);
					pin!(activity);
	
					loop {
						tokio::select! {
							_ = &mut activity => {},
							_ = notify.notified() => {
								if let Some(ref callback) = self.disconnect_callback { callback(Ok(())) }
								break;
							},
							Some(msg) = socket.next() => match msg {
								Ok(Some(msg)) => { if let Some(ref callback) = self.message_callback { callback(msg) } },
								Ok(None) => (),
								Err(e) => { 
									error!("{e:?}");
									info!("Terminating websocket");
									if let Some(ref callback) = self.disconnect_callback { callback(Err(e.into())) };
									break;
								}
							},
						}
					}
				}
			}
		});
		
		(notify, handle)
	}
}

async fn connect<'a>(client: &OFClient) -> Result<WebSocketClient<Connected<'a>>, DaemonError> {
	info!("Fetching user data");
	let me = client.get("https://onlyfans.com/api2/v2/users/me")
		.and_then(|response| response.json::<user::Me>())
		.inspect_err(|err| error!("Error fetching user data: {err}"))
		.await?;
	
	debug!("{me:?}");
	info!("Connecting as {}", me.name);
	let socket = WebSocketClient::new()
		.connect(&me.ws_url, &me.ws_auth_token)
		.inspect_err(|err| error!("Error connecting: {err}"))
		.await?;

	Ok(socket)
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

async fn simulate_activity(client: &OFClient) {
	let rng = StdRng::from_entropy();
	let mut intervals = rng.sample_iter(Exp1).map(|v: f32| Duration::from_secs_f32(v * 60.0));
	loop {
		sleep(intervals.next().unwrap()).await;
		let click = rand::random::<ClickStats>();
		trace!("Simulating site activity: {}", serde_json::to_string(&click).unwrap());
		let _ = client.post_json("https://onlyfans.com/api2/v2/users/clicks-stats", &click).await;
	}
}