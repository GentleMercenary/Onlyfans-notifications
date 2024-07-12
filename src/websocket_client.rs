use anyhow::bail;
use async_scoped::{spawner::use_tokio::Tokio, Scope, TokioScope};
use chrono::Utc;
use of_client::client::OFClient;
use serde::Serialize;
use crate::structs::socket;
use rand::{rngs::StdRng, SeedableRng, Rng};
use rand_distr::{Distribution, Exp1, Standard};
use std::time::Duration;
use futures::TryFutureExt;
use tokio::{net::TcpStream, time::timeout};
use futures_util::{SinkExt, StreamExt, Future};
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream, tungstenite::Message};

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
pub struct TimeoutExpired;
impl std::error::Error for TimeoutExpired {}
impl std::fmt::Display for TimeoutExpired {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str("Timeout expired")
	}
}

impl TryFrom<Message> for socket::Message {
	type Error = anyhow::Error;

	fn try_from(value: Message) -> Result<Self, <socket::Message as TryFrom<Message>>::Error> {
		let s = value.to_text()?;
		if !s.starts_with("{\"online\":[") { debug!("Received message: {s}") };

		serde_json::from_str::<socket::Message>(s)
		.inspect_err(|err| warn!("Message could not be parsed: {s}, reason: {err}"))
		.map_err(Into::into)
	}
}

pub struct Disconnected;
pub struct Connected {
	socket: WebSocketStream<MaybeTlsStream<TcpStream>>
}

pub struct WebSocketClient<Connection = Disconnected> {
	connection: Connection,
}

impl WebSocketClient {
	pub fn new() -> Self {
		Self { connection: Disconnected }
	}
}

impl Default for WebSocketClient {
	fn default() -> WebSocketClient {
		WebSocketClient::new()
	}
}

impl WebSocketClient<Disconnected> {
	pub async fn connect(self, url: &str, token: &str) -> anyhow::Result<WebSocketClient<Connected>> {
		info!("Creating websocket");
		let (socket, _) = connect_async(url).await?;
		info!("Websocket created");
		let mut connected_client = WebSocketClient { 
			connection: Connected { socket }
		};

		info!("Sending connect message");
		connected_client.connection.socket
		.send(serde_json::to_vec(&socket::Connect { act: "connect", token })?.into())
		.await?;

		connected_client
		.wait_for_message(Duration::from_secs(10))
		.and_then(|msg| async move {
			match msg {
				Some(socket::Message::Connected(msg)) => {
					info!("Connected message received: {:?}", msg); 
					Ok(())
				},
				_ => bail!("Unexpected response to connect request: {:?}", msg)
			}
		}).await?;

		Ok(connected_client)
	}
}

impl WebSocketClient<Connected> {
	pub async fn close(mut self) -> Result<(), tokio_tungstenite::tungstenite::Error> {
		self.connection.socket.close(None).await
	}

	pub async fn message_loop(&mut self, client: &OFClient, cancel: impl Future<Output = ()>) -> anyhow::Result<()> {
		info!("Starting websocket message loop");
		let mut interval = tokio::time::interval(Duration::from_secs(20));
		let mut heartbeat_flight = false;
		let rng = StdRng::from_entropy();
		let mut activity_interval = rng.sample_iter(Exp1).map(|v: f32| Duration::from_secs_f32(v * 60.0));
		let mut activity = tokio::time::interval(activity_interval.next().unwrap());

		let mut scope: TokioScope<'_, ()> = unsafe { Scope::create(Tokio) };
		tokio::pin!(cancel);
		
		let exit = loop {
			tokio::select! {
				_ = &mut cancel => break Ok(()),
				_ = activity.tick() => {
					let click = rand::random::<ClickStats>();
					trace!("Simulating site activity: {}", serde_json::to_string(&click)?);
					scope.spawn_cancellable(async move { let _ = client.post("https://onlyfans.com/api2/v2/users/clicks-stats", Some(&click)).await; }, || ());

					activity = tokio::time::interval(activity_interval.next().unwrap());
					activity.tick().await;
				},
				_ = interval.tick() => {
					self.send_heartbeat().await?;
					heartbeat_flight = true;
				},
				msg = self.wait_for_message(if heartbeat_flight { Duration::from_secs(5) } else { Duration::MAX }) => {
					match msg {
						Ok(Some(msg)) => {
							if let socket::Message::Onlines(_) = msg {
								trace!("Heartbeat acknowledged: {msg:?}");
								heartbeat_flight = false;
							}
							scope.spawn_cancellable(async move { let _ = msg.handle_message(client).await; }, || ());
						},
						Ok(None) => {},
						Err(err) => break Err(err),
					}
				}
			}
		};

		scope.cancel();
		exit

	}

	async fn send_heartbeat(&mut self) -> anyhow::Result<()> {
		const HEARTBEAT: socket::Heartbeat = socket::Heartbeat { act: "get_onlines", ids: &[] };

		trace!("Sending heartbeat: {HEARTBEAT:?}");
		self.connection.socket
		.send(Message::Binary(serde_json::to_vec(&HEARTBEAT)?))
		.await
		.map_err(Into::into)
	}

	async fn wait_for_message(&mut self, duration: Duration) -> anyhow::Result<Option<socket::Message>> {
		match timeout(duration, self.connection.socket.next()).await {
			Err(_) => bail!(TimeoutExpired),
			Ok(None) => bail!("Message queue exhausted"),
			Ok(Some(msg)) => msg.map(|msg| msg.try_into().ok()).map_err(Into::into)
		}
	}
}