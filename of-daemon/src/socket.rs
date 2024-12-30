#![allow(dead_code)]

use crate::structs;
use thiserror::Error;
use std::{sync::Arc, task::Poll, time::Duration};
use futures::{future::BoxFuture, stream::BoxStream, FutureExt, Stream};
use tokio::{sync::Notify, time::{error::Elapsed, interval, timeout}};
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::{connect_async, tungstenite::{self, Message}};

#[derive(Error, Debug)]
pub enum SocketError {
	#[error("{0}")]
	Socket(#[from] tungstenite::Error),
	#[error("Timeout expired")]
	TimeoutExpired,
	#[error("Unexpected message")]
	UnexpectedMessage
}

impl From<Elapsed> for SocketError {
	fn from(_value: Elapsed) -> Self { SocketError::TimeoutExpired }
}

impl structs::Message {
	fn decode(value: Message) -> Option<Self> {
		let s = value.to_text().ok()?;
		if !s.starts_with("{\"online\":[") { debug!("Received message: {s}") }
		else { trace!("Received message: {s}") }

		serde_json::from_str(s)
		.inspect_err(|err| warn!("Message could not be parsed: {s}, reason: {err}"))
		.ok()
	}
}

pub struct Disconnected;
pub struct Connected<'a> {
	heartbeat_fut: BoxFuture<'a, Result<(), SocketError>>,
	message_fut: BoxStream<'a, Result<Option<structs::Message>, tungstenite::Error>>,
}

pub struct WebSocketClient<State = Disconnected> {
	state: State,
}

impl WebSocketClient {
	pub const fn new() -> Self {
		Self { state: Disconnected }
	}
}

impl WebSocketClient<Disconnected> {
	pub async fn connect<'a>(self, url: &str, token: &str) -> Result<WebSocketClient<Connected<'a>>, SocketError> {
		info!("Creating websocket");
		let (socket, resp) = connect_async(url).await?;
		info!("Websocket created: {:#?}", resp);

		let (mut sink, stream) = socket.split();

		info!("Sending connect message");
		sink.send(serde_json::to_vec(&structs::Connect { act: "connect", token }).unwrap().into())
		.await?;
	
		let notify = Arc::new(Notify::new());
		let heartbeat_fut = {
			let ack = notify.clone();
			
			async move {
				let heartbeat = serde_json::to_string(&structs::Heartbeat { act: "get_onlines", ids: &[] }).unwrap();
				let mut interval = interval(Duration::from_secs(20));
				loop {
					let _ = interval.tick().await;
			
					trace!("Sending heartbeat: {heartbeat:?}");
					if let Err(e) = sink.send(Message::from(heartbeat.as_str())).await {
						break Err(e.into());
					}
			
					match timeout(Duration::from_secs(5), ack.notified()).await {
						Ok(_) => trace!("Heartbeat acknowledged"),
						Err(_) => break Err(SocketError::TimeoutExpired),
					}
				}
			}
			.boxed()
		};
		
		let mut message_fut = stream
			.map(move |rc| 
				rc.map(|msg| 
					structs::Message::decode(msg)
					.inspect(|message| if let structs::Message::Onlines(_) = message {
						notify.notify_one();
					})
				)
			)
			.boxed();

		match timeout(Duration::from_secs(10), message_fut.next().map(|v| v.unwrap())).await {
			Ok(Ok(Some(structs::Message::Connected(msg)))) => {
				info!("Connected message received: {:?}", msg); 
				Ok(())
			}
			Err(_) => Err(SocketError::TimeoutExpired),
			Ok(Err(e)) => Err(e.into()),
			Ok(Ok(_)) => Err(SocketError::UnexpectedMessage)
		}?;

		Ok(WebSocketClient {
			state: Connected {
				heartbeat_fut,
				message_fut
			}
		})
	}
}

impl WebSocketClient<Connected<'_>> {
	pub fn close(self) -> WebSocketClient<Disconnected> {
		WebSocketClient { state: Disconnected }
	}
}

impl Stream for WebSocketClient<Connected<'_>> {
	type Item = Result<Option<structs::Message>, SocketError>;

	fn poll_next(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Option<Self::Item>> {
		let this = self.get_mut();

		let message_poll = this.state.message_fut.poll_next_unpin(cx);

		if let Poll::Ready(Err(err)) = this.state.heartbeat_fut.poll_unpin(cx) {
			return Poll::Ready(Some(Err(err)))
		}

		match message_poll {
			Poll::Ready(val) => Poll::Ready(val.map(|inner| inner.map_err(Into::into))),
			Poll::Pending => Poll::Pending
		}
	}
}