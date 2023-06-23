use anyhow::bail;
use std::time::Duration;
use futures::TryFutureExt;
use futures_util::{SinkExt, StreamExt, stream::{SplitStream, SplitSink}};
use tokio::{net::TcpStream, time::timeout};
use crate::{structs::{self, MessageType}, client::{OFClient, Authorized}};
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream, tungstenite::Message};

impl TryFrom<Message> for MessageType {
    type Error = anyhow::Error;

    fn try_from(value: Message) -> Result<Self, <MessageType as TryFrom<Message>>::Error> {
        let s = value.to_text()?;
		if !s.starts_with("{\"online\":[") { debug!("Received message: {s}") };

		serde_json::from_str::<structs::MessageType>(s)
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
	pub async fn connect(&mut self, token: &str, client: &OFClient<Authorized>) -> anyhow::Result<WebSocketClient<Connected>> {
		info!("Creating websocket");
		let (socket, _) = connect_async("wss://ws2.onlyfans.com/ws2/").await?;
		info!("Websocket created");

		let(sink, stream) = socket.split();

		let mut connected_client = WebSocketClient { 
			connection: Connected { sink, stream }
		};

		info!("Sending connect message");
		connected_client.connection.sink
		.send(serde_json::to_vec(&structs::ConnectMessage { act: "connect", token })?.into())
		.await?;

		connected_client
		.wait_for_message(Duration::from_secs(10))
		.and_then(|msg| async move {
			match msg {
				Some(msg @ structs::MessageType::Connected(_)) => msg.handle_message(client).await,
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

	pub async fn message_loop(&mut self, client: OFClient<Authorized>) -> anyhow::Result<()> {
		info!("Starting websocket message loop");
		let mut interval = tokio::time::interval(Duration::from_secs(20));
		let mut heartbeat_flight = false;

		loop {
			tokio::select! {
				_ = interval.tick() => {
					self.send_heartbeat().await?;
					heartbeat_flight = true;
				},
				msg = self.wait_for_message(if heartbeat_flight { Duration::from_secs(5) } else { Duration::MAX }) => {
					match msg {
						Ok(Some(msg)) => {
							if let structs::MessageType::Onlines(_) = msg {
								debug!("Heartbeat acknowledged");
								heartbeat_flight = false;
							}
							msg.handle_message(&client).await?;
						},
						Ok(None) => {},
						Err(err) => return Err(err),
					}
				}
			}
		}
	}

	async fn send_heartbeat(&mut self) -> anyhow::Result<()> {
		debug!("Sending heartbeat");
		self.connection.sink
		.send(Message::Binary(serde_json::to_vec(&structs::HeartbeatMessage::default())?))
		.await
		.map_err(Into::into)
	}

	async fn wait_for_message(&mut self, duration: Duration) -> anyhow::Result<Option<structs::MessageType>> {
		match timeout(duration, self.connection.stream.next()).await {
			Err(_) => bail!("Timeout expired"),
			Ok(None) => bail!("Message queue exhausted"),
			Ok(Some(msg)) => msg.map(|msg| msg.try_into().ok()).map_err(Into::into)
		}
	}
}