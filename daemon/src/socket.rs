#![allow(dead_code)]

use crate::structs;
use thiserror::Error;
use std::{sync::{Arc, LazyLock}, time::Duration};
use futures::stream::SplitStream;
use tokio::{net::TcpStream, sync::Notify, time::{sleep_until, timeout, Instant}};
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::{connect_async, tungstenite::{self, Message}, MaybeTlsStream, WebSocketStream};

#[derive(Error, Debug)]
pub enum SocketError {
	#[error("{0}")]
	SocketError(#[from] tungstenite::Error),
	#[error("Timeout expired")]
	TimeoutExpired,
	#[error("Unexpected message")]
	UnexpectedMessage
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

static HEARTBEAT: LazyLock<Message> = LazyLock::new(|| {
	Message::from(
		serde_json::to_vec(
			&structs::Heartbeat { act: "get_onlines", ids: &[] }
		).unwrap()
	)
});

pub struct Disconnected;
pub struct Connected {
	heartbeat_handle: tokio::task::JoinHandle<()>,
	message_handle: tokio::task::JoinHandle<()>,
}

pub struct WebSocketClient<State = Disconnected> {
	response_callback: Option<Arc<dyn Fn(SocketResponse) + Sync + Send + 'static>>,
	state: State,
}

impl WebSocketClient {
	pub const fn new() -> Self {
		Self { state: Disconnected, response_callback: None }
	}

	pub fn on_response(mut self, f: impl Fn(SocketResponse) + Sync + Send + 'static) -> Self {
		self.response_callback = Some(Arc::new(f));
		self
	}
}

impl Default for WebSocketClient {
	fn default() -> Self {
		Self::new()
	}
}

impl WebSocketClient<Disconnected> {
	pub async fn connect(self, url: &str, token: &str) -> Result<WebSocketClient<Connected>, SocketError> {
		info!("Creating websocket");
		let (socket, _) = connect_async(url).await?;
		info!("Websocket created");

		let (mut sink, mut stream) = socket.split();

		info!("Sending connect message");
		
		sink
		.send(serde_json::to_vec(&structs::Connect { act: "connect", token }).unwrap().into())
		.await?;
	

		match timeout(Duration::from_secs(10), wait_for_message(&mut stream)).await {
			Ok(Ok(Some(structs::Message::Connected(msg)))) => {
				info!("Connected message received: {:?}", msg); 
				Ok(())
			}
			Err(_) => Err(SocketError::TimeoutExpired),
			Ok(Err(e)) => Err(e.into()),
			Ok(Ok(_)) => Err(SocketError::UnexpectedMessage)
		}?;

		let heartbeat_notify = Arc::new(Notify::new());
		
		let heartbeat_handle = {
			let heartbeat_notify = heartbeat_notify.clone();
			let callback = self.response_callback.clone();
			tokio::spawn(async move {
				loop {
					let last_send_time = Instant::now();
					trace!("Sending heartbeat: {HEARTBEAT:?}");
					if let Err(e) = sink.send(HEARTBEAT.clone()).await {
						error!("{e:?}");
						if let Some(callback) = callback { callback(Err(e.into())); };
						break;
					}
					
					match timeout(Duration::from_secs(5), heartbeat_notify.notified()).await {
						Ok(_) => trace!("Heartbeat acknowledged"),
						Err(_) => {
							if let Some(callback) = callback {
								let e = SocketError::TimeoutExpired;
								error!("{e:?}");
								callback(Err(e));
							};
							break;
						}
					}
	
					sleep_until(last_send_time + Duration::from_secs(20)).await;
				}
			})
		};

		let message_handle = {
			let callback = self.response_callback.clone();
			tokio::spawn(async move {
				loop {
					match wait_for_message(&mut stream).await {
						Ok(Some(msg)) => {
							if let structs::Message::Onlines(_) = msg { heartbeat_notify.notify_one(); }
							else if let Some(ref callback) = callback { callback(Ok(msg)); }
						},
						Err(e) => { 
							error!("{e:?}");
							if let Some(callback) = callback { callback(Err(e.into())); };
							break;
						}
						Ok(None) => (),
					}
				}
			})
		};

		Ok(WebSocketClient {
			state: Connected {
				heartbeat_handle,
				message_handle
			},
			response_callback: self.response_callback
		})
	}
}

impl WebSocketClient<Connected> {
	pub fn close(self) -> WebSocketClient<Disconnected> {
		drop(self.state);
		WebSocketClient { state: Disconnected, response_callback: self.response_callback }
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