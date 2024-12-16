#[macro_use]
extern crate log;

pub mod deserializers;
pub mod structs;
pub mod socket;

pub use tokio_tungstenite::tungstenite::error::{Error as WSError, ProtocolError};

use std::{sync::Arc, time::Duration};
use chrono::Utc;
use futures::TryFutureExt;
use of_client::{OFClient, reqwest, user};
use rand::{rngs::StdRng, Rng, SeedableRng};
use rand_distr::{Distribution, Exp1, Standard};
use serde::Serialize;
use thiserror::Error;
use tokio::{task::JoinHandle, time::sleep};
use crate::{socket::{Connected, SocketError, SocketResponse, WebSocketClient}, structs::Message};

#[derive(Error, Debug)]
pub enum DaemonError {
	#[error("{0}")]
	Socket(#[from] SocketError),
	#[error("{0}")]
	Request(#[from] reqwest::Error)
}

struct Handles {
	activity_handle: JoinHandle<()>,
	#[allow(dead_code)] socket: WebSocketClient<Connected>
}

impl Drop for Handles {
	fn drop(&mut self) {
		self.activity_handle.abort();
	}
}

pub struct SocketDaemon {
	handles: Option<Handles>,
	message_callback: Option<Arc<dyn Fn(Message) + Sync + Send>>,
	disconnect_callback: Option<Arc<dyn Fn(DaemonError) + Sync + Send>>,
}

impl SocketDaemon {
	pub fn new() -> Self {
		Self {
			handles: None,
			message_callback: None,
			disconnect_callback: None
		}
	}

	pub fn on_message(mut self, f: impl Fn(Message) + Sync + Send + 'static) -> Self {
		self.message_callback = Some(Arc::new(f));
		self
	}

	pub fn on_disconnect(mut self, f: impl Fn(DaemonError) + Sync + Send + 'static) -> Self {
		self.disconnect_callback = Some(Arc::new(f));
		self
	}

	pub async fn start(&mut self, client: OFClient) -> Result<(), DaemonError> {
		info!("Fetching user data");
		let me = client.get("https://onlyfans.com/api2/v2/users/me")
			.and_then(|response| response.json::<user::Me>())
			.inspect_err(|err| error!("Error fetching user data: {err}"))
			.await?;
		
		debug!("{me:?}");
		info!("Connecting as {}", me.name);
		
		let activity_handle = tokio::spawn({
			async move {
				let rng = StdRng::from_entropy();
				let mut intervals = rng.sample_iter(Exp1).map(|v: f32| Duration::from_secs_f32(v * 60.0));
				
				loop {
					sleep(intervals.next().unwrap()).await;
					let click = rand::random::<ClickStats>();
					trace!("Simulating site activity: {}", serde_json::to_string(&click).unwrap());
					let _ = client.post_json("https://onlyfans.com/api2/v2/users/clicks-stats", &click).await;
				}
			}
		});
		
		let response_callback = {
			let message_callback = self.message_callback.clone();
			let disconnect_callback = self.disconnect_callback.clone();
			move |response: SocketResponse| {
				match response {
					Ok(msg) => if let Some(callback) = &message_callback {
						callback(msg);
					},
					Err(e) => {
						info!("Terminating websocket");
						if let Some(callback) = &disconnect_callback {
							callback(e.into());
						}
					}
				}
			}
		};

		let socket = WebSocketClient::new()
			.on_response(response_callback)
			.connect(&me.ws_url, &me.ws_auth_token)
			.inspect_err(|err| error!("Error connecting: {err}"))
			.await?;
	
		self.handles = Some(Handles {
			activity_handle,
			socket
		});

		Ok(())
	}

	pub fn stop(&mut self) {
		self.handles = None;
	}

	pub fn running(&self) -> bool {
		self.handles.is_some()
	}
}

impl Drop for SocketDaemon {
	fn drop(&mut self) {
		self.stop();
	}
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