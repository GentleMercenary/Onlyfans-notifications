use super::message_types::{self, Handleable};

use anyhow::{anyhow, bail};
use futures_util::{SinkExt, StreamExt};
use std::time::Duration;
use tokio::{net::TcpStream, time::sleep};
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};

pub struct WebSocketClient {
	socket: Option<WebSocketStream<MaybeTlsStream<TcpStream>>>,
	heartbeat: Message,
}

impl WebSocketClient {
	pub fn new() -> anyhow::Result<Self> {
		Ok(Self {
			socket: None,
			heartbeat: Message::Text(serde_json::to_string(&message_types::GetOnlinesMessage {
				act: "get_onlines",
				ids: &[],
			})?),
		})
	}

	pub async fn close(&mut self) -> tokio_tungstenite::tungstenite::Result<()> {
		if let Some(socket) = self.socket.as_mut() {
			socket.close(None).await?;
		}
		Ok(())
	}

	pub async fn connect(&mut self, token: &str) -> anyhow::Result<&mut Self> {
		info!("Creating websocket");
		let (ws, _) = connect_async("wss://ws2.onlyfans.com/ws2/").await?;
		info!("Websocket created");
		self.socket = Some(ws);

		let mut success = false;
		if let Some(socket) = self.socket.as_mut() {
			info!("Sending connect message");

			socket
				.send(
					serde_json::to_string(&message_types::ConnectMessage {
						act: "connect",
						token,
					})?
					.into(),
				)
				.await?;

			let timeout = sleep(Duration::from_secs(30));
			tokio::pin!(timeout);

			tokio::select! {
				msg = self.wait_for_message() => {
					if let Some(msg) = msg? {
						match msg {
							message_types::MessageType::Connected(_) => {
								if msg.handle_message().await.is_ok() {
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
		}

		if success {
			Ok(self)
		} else {
			bail!("Couldn't connect to websocket")
		}
	}

	pub async fn message_loop(&mut self) -> anyhow::Result<()> {
		info!("Starting websocket message loop");
		let mut interval = tokio::time::interval(Duration::from_secs(20));
		let mut msg_received = true;

		loop {
			tokio::select! {
				msg = self.wait_for_message() => {
					if let Some(msg) = msg? {
						let _ = msg.handle_message().await;
					}
					msg_received = true;
				},
				_ = interval.tick() => {
					if !msg_received {
						error!("Timeout expired");
						break;
					}

					let writer = self.socket.as_mut().ok_or_else(|| anyhow!(""))?;
					writer.send(self.heartbeat.clone()).await?;
					msg_received = false;
				}
			}
		}

		Err(anyhow!("Message loop interruped unexpectedly"))
	}

	async fn wait_for_message(&mut self) -> anyhow::Result<Option<message_types::MessageType>> {
		let reader = self.socket.as_mut().unwrap();
		let msg = reader
			.next()
			.await
			.ok_or_else(|| anyhow!("Message queue exhausted"))
			??;

		msg.to_text()
			.map_err(|err| err.into())
			.inspect(|s| {
				if *s != "{\"online\":[]}" {
					debug!("Received message: {s}")
				}
			})
			.map(|s| serde_json::from_str::<message_types::MessageType>(s).ok())
	}
}
