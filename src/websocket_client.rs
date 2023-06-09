use crate::{structs, client::{OFClient, Authorized}};

use anyhow::{anyhow, bail, Ok};
use futures_util::{SinkExt, StreamExt};
use std::time::Duration;
use tokio::{net::TcpStream, time::sleep};
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};

pub struct Disconnected;
pub struct Connected {
	socket: WebSocketStream<MaybeTlsStream<TcpStream>>,
	heartbeat: Message
}

pub struct WebSocketClient<Connection> {
	connection: Connection,
}

impl WebSocketClient<Disconnected> {
	pub fn new() -> Self {
		Self { connection: Disconnected }
	}

	pub async fn connect(&mut self, token: &str, client: &OFClient<Authorized>) -> anyhow::Result<WebSocketClient<Connected>> {
		info!("Creating websocket");
		let (ws, _) = connect_async("wss://ws2.onlyfans.com/ws2/").await?;
		info!("Websocket created");

		let mut connected_client = WebSocketClient { 
			connection: Connected {
				socket: ws,
				heartbeat: Message::Text(serde_json::to_string(&structs::GetOnlinesMessage {
					act: "get_onlines",
					ids: &[],
				})?),
			}
		};

		let mut success = false;
			info!("Sending connect message");

			connected_client.connection.socket.send(
				serde_json::to_string(&structs::ConnectMessage {
					act: "connect",
					token,
				})?
				.into(),
			)
			.await?;

			let timeout = sleep(Duration::from_secs(30));
			tokio::pin!(timeout);

			tokio::select! {
				msg = connected_client.wait_for_message() => {
					if let Some(msg) = msg? {
						match msg {
							structs::MessageType::Connected(_) => {
								if msg.handle_message(client).await.is_ok() {
									success = true;
								}
							},
							_ => { error!("Unexpected response to connect request: {:?}", msg); }
						}
					}
				},
				_ = &mut timeout => {
					// Heartbeat wasn't sent in time or no response was received in time
					error!("Timeout expired");
				}
			};

		if success {
			Ok(connected_client)

		} else {
			bail!("Couldn't connect to websocket")
		}
	}
}

impl WebSocketClient<Connected> {
	pub async fn close(&mut self) -> tokio_tungstenite::tungstenite::Result<()> {
		self.connection.socket.close(None).await
	}

	pub async fn message_loop(&mut self, client: &OFClient<Authorized>) -> anyhow::Result<()> {
		info!("Starting websocket message loop");
		let mut interval = tokio::time::interval(Duration::from_secs(20));
		let mut msg_received = true;

		loop {
			tokio::select! {
				msg = self.wait_for_message() => {
					if let Some(msg) = msg? {
						let _ = msg.handle_message(client).await;
					}
					msg_received = true;
				},
				_ = interval.tick() => {
					if !msg_received {
						error!("Timeout expired");
						break;
					}

					self.connection.socket.send(self.connection.heartbeat.clone()).await?;
					msg_received = false;
				}
			}
		}

		Err(anyhow!("Message loop interruped unexpectedly"))
	}

	async fn wait_for_message(&mut self) -> anyhow::Result<Option<structs::MessageType>> {
		let msg = self.connection.socket
			.next()
			.await
			.ok_or_else(|| anyhow!("Message queue exhausted"))??;

		msg.to_text()
			.map_err(Into::into)
			.map(|s| {
				(!s.starts_with("{\"online\":[")).then(|| {
					debug!("Received message: {s}");
					serde_json::from_str::<structs::MessageType>(s)
					.inspect_err(|err| warn!("Message could not be parsed: {s}, reason: {err}"))
					.ok()
				}).flatten()
			})
	}
}