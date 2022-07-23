mod message_types;
mod client;


use tokio::io::Result;
use std::time::Duration;
use reqwest::Url;
use tokio_tungstenite::connect_async;
use futures_util::{SinkExt, StreamExt};
use std::collections::HashSet;

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

		let mut unserialisables: HashSet<String> = HashSet::new();
	
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
									Err(e) => {
										if !unserialisables.contains(s) {
											println!("Error deserializing struct \"{}\", Error \"{}\"", s, e);
											unserialisables.insert(s.to_owned());
										}
									}
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

#[cfg(test)]
mod tests {
	use super::*;

	#[tokio::test]
    async fn handle_chat_message() {
        let incoming = r#"{
            "api2_chat_message": {
                "text": "This is a test message",
                "fromUser": {
                    "avatar": "https://public.onlyfans.com/files/y/yf/yfh/yfhvrttzhvmemj1hforykjvtgpztkzk51579803429/avatar.jpg",
                    "id": 19526127,
                    "name": "Mikomi Hokina",
                    "username": "mikomihokina" } 
                } 
            }"#;

        match serde_json::from_str::<message_types::MessageType>(&incoming) {
            Ok(msg) => msg.handle_message().await,
            _ => panic!("Did not parse to correct type")
        }
    }
}