#![allow(dead_code)]

use anyhow::bail;
use thiserror::Error;
use crate::structs;
use std::{sync::{Arc, LazyLock}, time::Duration};
use futures::stream::SplitStream;
use tokio::{net::TcpStream, sync::{mpsc::{self, UnboundedReceiver}, Notify}, time::{sleep_until, timeout, Instant}};
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::{connect_async, tungstenite::{self, Message}, MaybeTlsStream, WebSocketStream};

#[derive(Error, Debug)]
pub enum SocketError {
	#[error("{0}")]
	SocketError(#[from] tungstenite::Error),
	#[error("Timeout expired")]
	TimeoutExpired,
}

pub type SocketResponse = Result<structs::Message, SocketError>;

#[derive(Error, Debug)]
pub enum DecodeError {
	#[error("{0}")]
	SocketError(#[from] tungstenite::Error),
	#[error("{0}")]
	DecodeError(#[from] serde_json::Error),
}

impl TryFrom<Message> for structs::Message {
	type Error = DecodeError;

	fn try_from(value: Message) -> Result<Self, <Self as TryFrom<Message>>::Error> {
		let s = value.to_text()?;
		if !s.starts_with("{\"online\":[") { debug!("Received message: {s}") }
		else { trace!("Received message: {s}") }

		serde_json::from_str::<Self>(s)
		.inspect_err(|err| warn!("Message could not be parsed: {s}, reason: {err}"))
		.map_err(Into::into)
	}
}

static HEARTBEAT: LazyLock<Vec<u8>> = LazyLock::new(|| {
	serde_json::to_vec(&structs::Heartbeat { act: "get_onlines", ids: &[] }).unwrap()
});

pub struct Disconnected;
#[derive(Debug)]
pub struct Connected {
	heartbeat_handle: tokio::task::JoinHandle<()>,
	message_handle: tokio::task::JoinHandle<()>,
}

#[derive(Debug)]
pub struct WebSocketClient<State = Disconnected> {
	state: State,
}

impl WebSocketClient {
	pub const fn new() -> Self {
		Self { state: Disconnected }
	}
}

impl Default for WebSocketClient {
	fn default() -> Self {
		Self::new()
	}
}

impl WebSocketClient<Disconnected> {
	pub async fn connect(self, url: &str, token: &str) -> anyhow::Result<(WebSocketClient<Connected>, UnboundedReceiver<SocketResponse>)> {
		info!("Creating websocket");
		let (socket, _) = connect_async(url).await?;
		info!("Websocket created");

		let (mut sink, mut stream) = socket.split();

		info!("Sending connect message");
		
		sink
		.send(serde_json::to_vec(&structs::Connect { act: "connect", token })?.into())
		.await?;
	

		match timeout(Duration::from_secs(10), wait_for_message(&mut stream)).await {
			Ok(Ok(Some(structs::Message::Connected(msg)))) => {
				info!("Connected message received: {:?}", msg); 
				Ok(())
			}
			Err(_) => Err(SocketError::TimeoutExpired),
			Ok(Err(e)) => Err(e.into()),
			Ok(Ok(v)) => {
				bail!("Invalid response to connect message: {v:?}")
			}
		}?;

		let (message_tx, message_rx) = mpsc::unbounded_channel::<SocketResponse>();
		let heartbeat_notify = Arc::new(Notify::new());
		
		let heartbeat_handle = {
			let message_tx = message_tx.clone();
			let heartbeat_notify = heartbeat_notify.clone();
			tokio::spawn(async move {
				loop {
					let last_send_time = Instant::now();
					trace!("Sending heartbeat: {HEARTBEAT:?}");
					if let Err(e) = sink.send(HEARTBEAT.as_slice().into()).await {
						error!("{e:?}");
						let _ = message_tx.send(Err(e.into()));
						break;
					}
	
					match timeout(Duration::from_secs(5), heartbeat_notify.notified()).await {
						Ok(_) => trace!("Heartbeat acknowledged"),
						Err(_) => {
							let _ = message_tx.send(Err(SocketError::TimeoutExpired));
							break;
						}
					}
	
					sleep_until(last_send_time + Duration::from_secs(20)).await;
				}
			})
		};

		let message_handle = {
			tokio::spawn(async move {
				loop {
					match wait_for_message(&mut stream).await {
						Ok(Some(msg)) => {
							if let structs::Message::Onlines(_) = msg { heartbeat_notify.notify_one(); }
							else { let _ = message_tx.send(Ok(msg)); }
						},
						Err(e) => { 
							error!("{e:?}");
							let _ = message_tx.send(Err(e.into()));
							break;
						}
						Ok(None) => (),
					}
				}
			})
		};

		Ok((WebSocketClient { 
			state: Connected {
				heartbeat_handle,
				message_handle
			}
		}, message_rx))
	}
}

impl WebSocketClient<Connected> {
	pub fn close(self) -> WebSocketClient<Disconnected> {
		drop(self.state);
		WebSocketClient { state: Disconnected }
	}
}

impl Drop for Connected {
	fn drop(&mut self) {
		self.heartbeat_handle.abort();
		self.message_handle.abort();
	}
}

async fn wait_for_message(stream: &mut SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>) -> tungstenite::Result<Option<structs::Message>> {
	stream.next()
	.await
	.unwrap()
	.map(|msg| msg.try_into().ok())
}