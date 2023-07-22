use crate::{client::{OFClient, Authorized}, structs::{socket, ClickStats}};

use anyhow::bail;
use rand::{rngs::StdRng, SeedableRng, Rng};
use rand_distr::Exp1;
use std::time::Duration;
use futures::TryFutureExt;
use tokio::{net::TcpStream, time::timeout};
use futures_util::{SinkExt, StreamExt, stream::{SplitStream, SplitSink}};
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream, tungstenite::Message};

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
	sink: SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
	stream: SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>
}

pub struct WebSocketClient<Connection = Disconnected> {
	connection: Connection,
}

impl WebSocketClient {
	pub fn new() -> Self {
		Self { connection: Disconnected }
	}
}

impl WebSocketClient<Disconnected> {
	pub async fn connect(self, token: &str, client: &OFClient<Authorized>) -> anyhow::Result<WebSocketClient<Connected>> {
		info!("Creating websocket");
		let (socket, _) = connect_async("wss://ws2.onlyfans.com/ws2/").await?;
		info!("Websocket created");

		let(sink, stream) = socket.split();

		let mut connected_client = WebSocketClient { 
			connection: Connected { sink, stream }
		};

		info!("Sending connect message");
		connected_client.connection.sink
		.send(serde_json::to_vec(&socket::Connect { act: "connect", token })?.into())
		.await?;

		connected_client
		.wait_for_message(Duration::from_secs(10))
		.and_then(|msg| async move {
			match msg {
				Some(msg @ socket::Message::Connected(_)) => msg.handle_message(client).await,
				_ => bail!("Unexpected response to connect request: {:?}", msg)
			}
		}).await?;

		Ok(connected_client)
	}
}

impl WebSocketClient<Connected> {
	pub async fn close(self) -> anyhow::Result<()> {
		let mut socket = self.connection.stream.reunite(self.connection.sink)?;
		socket.close(None).await?;
		Ok(())
	}

	pub async fn message_loop(&mut self, client: &OFClient<Authorized>) -> anyhow::Result<()> {
		info!("Starting websocket message loop");
		let mut interval = tokio::time::interval(Duration::from_secs(20));
		let mut heartbeat_flight = false;
		let rng = StdRng::from_entropy();
		let mut activity_interval = rng.sample_iter(Exp1).map(|v: f32| Duration::from_secs_f32(v * 60.0));
		let mut activity = tokio::time::interval(activity_interval.next().unwrap());
		activity.tick().await;

		loop {
			tokio::select! {
				_ = activity.tick() => {
					let click = rand::random::<ClickStats>();
					debug!("Simulating site activity: {}", serde_json::to_string(&click)?);
					if let Err(err) = client.post("https://onlyfans.com/api2/v2/users/clicks-stats", Some(&click)).await {
						warn!("{err:?}");
					}
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
								debug!("Heartbeat acknowledged: {msg:?}");
								heartbeat_flight = false;
							}
							msg.handle_message(client).await?;
						},
						Ok(None) => {},
						Err(err) => return Err(err),
					}
				}
			}
		}
	}

	async fn send_heartbeat(&mut self) -> anyhow::Result<()> {
		const HEARTBEAT: socket::Heartbeat = socket::Heartbeat { act: "get_onlines", ids: &[] };

		debug!("Sending heartbeat: {HEARTBEAT:?}");
		self.connection.sink
		.send(Message::Binary(serde_json::to_vec(&HEARTBEAT)?))
		.await
		.map_err(Into::into)
	}

	async fn wait_for_message(&mut self, duration: Duration) -> anyhow::Result<Option<socket::Message>> {
		match timeout(duration, self.connection.stream.next()).await {
			Err(_) => bail!(TimeoutExpired),
			Ok(None) => bail!("Message queue exhausted"),
			Ok(Some(msg)) => msg.map(|msg| msg.try_into().ok()).map_err(Into::into)
		}
	}
}