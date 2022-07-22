mod message_types;
mod client;


use tokio::io::Result;
use std::time::Duration;
use reqwest::Url;
use tokio_tungstenite::connect_async;
use futures_util::{SinkExt, StreamExt};

#[tokio::main]
async fn main() -> Result<()> {
	let auth_link: &str = "https://onlyfans.com/api2/v2/users/me";
	let response = client::get_json(auth_link).await.unwrap();
	
	if response.status().is_success() {
		let json_response: serde_json::Value = serde_json::from_str(&response.text().await.unwrap()).unwrap();

		let (ws, _) = connect_async(Url::parse("wss://ws.onlyfans.com/ws2/").unwrap()).await.expect("Failed to connect");
		let (mut writer, mut reader) = ws.split();
		let mut interval = tokio::time::interval(Duration::from_secs(20));
		
		writer.send(serde_json::to_string(
			&message_types::ConnectMessage {
				act: "connect",
				token: json_response["wsAuthToken"].as_str().unwrap()
			}).unwrap().into()).await.expect("Failed to establish connection");
	
		loop {
			tokio::select! {
				msg = reader.next() => {
					match msg {
						Some(msg) => {
							let msg = msg.unwrap();
							let s: &str = msg.to_text().unwrap();
							if msg.is_text() {
								match serde_json::from_str(s) as serde_json::Result<message_types::MessageType> {
									Ok(m) => m.handle_message().await,
									Err(_) => ()
								};
							} else if msg.is_close() {
								break;
							}
						}
						None => break,
					}
				},
				_ = interval.tick() => {
					writer.send(serde_json::to_string(
						&message_types::GetOnlinesMessage {
							act: "get_onlines",
							ids: &[]
						}).unwrap().into()).await.expect("Failed to send heartbeat, restart program");
				}
			}
		}
	} else {
		println!("{:?}", response);
		println!("{}", response.text().await.unwrap());
	}

	Ok(())
}