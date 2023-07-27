mod init;

use of_notifier::{get_auth_params, structs::socket, init, SETTINGS, settings::{Settings, Whitelist, GlobalSelection}};
use of_client::client::OFClient;
use std::thread::sleep;
use std::time::Duration;
use std::sync::Once;
use tokio::sync::Mutex;
use init::init_log;

static INIT: Once = Once::new();

fn test_init() {
	INIT.call_once(|| {
		init().unwrap();

		SETTINGS
		.set(Mutex::new(Settings {
			notify: Whitelist::Global(GlobalSelection::Full(true)),
			..Settings::default()
		}))
		.unwrap();

		init_log();
	});
}

macro_rules! socket_test {
	($name: ident, $incoming: expr, $match: pat) => {
		#[tokio::test]
		async fn $name() {
			test_init();

			let msg = serde_json::from_str::<socket::Message>($incoming).unwrap();
			assert!(matches!(msg, $match));
	
			let params = get_auth_params().unwrap();
			let client = OFClient::new().authorize(params).await.unwrap();
			msg.handle_message(&client).await.unwrap();
			sleep(Duration::from_millis(1000));
		}
	};
}

socket_test!(test_chat_message, r#"{
	"api2_chat_message": {
		"id": 0,
		"text": "This is a message<br />\n to test <a href = \"/onlyfans\">MARKDOWN parsing</a> 👌<br />\n in notifications 💯",
		"price": 3.99,
		"fromUser": {
			"avatar": "https://public.onlyfans.com/files/m/mk/mka/mkamcrf6rjmcwo0jj4zoavhmalzohe5a1640180203/avatar.jpg",
			"id": 15585607,
			"name": "OnlyFans",
			"username": "onlyfans"
		},
		"media": [
			{
				"id": 0,
				"canView": true,
				"src": "https://raw.githubusercontent.com/allenbenz/winrt-notification/main/resources/test/chick.jpeg",
				"preview": "https://raw.githubusercontent.com/allenbenz/winrt-notification/main/resources/test/flower.jpeg",
				"type": "photo"
			}
		]
	}
}"#, socket::Message::Tagged(socket::TaggedMessage::Api2ChatMessage(_)));

socket_test!(test_post_message,  r#"{
	"post_published": {
		"id": "129720708",
		"user_id" : "15585607",
		"show_posts_in_feed":true
	}
}"#, socket::Message::Tagged(socket::TaggedMessage::PostPublished(_)));

socket_test!(test_story_message, r#"{
	"stories": [
		{
			"id": 0,
			"userId": 15585607,
			"media":[
				{
					"id": 0,
					"canView": true,
					"files": {
						"source": {
							"url": "https://raw.githubusercontent.com/allenbenz/winrt-notification/main/resources/test/chick.jpeg"
						},
						"preview": {
							"url": "https://raw.githubusercontent.com/allenbenz/winrt-notification/main/resources/test/flower.jpeg"
						}
					},
					"type": "photo"
				}
			]
		}
	]
}"#, socket::Message::Tagged(socket::TaggedMessage::Stories(_)));

socket_test!(test_notification_message, r#"{
	"new_message":{
		"id":"0",
		"type":"message",
		"text":"is currently running a promotion, <a href=\"https://onlyfans.com/onlyfans\">check it out</a>",
		"subType":"promoreg_for_expired",
		"user_id":"274000171",
		"isRead":false,
		"canGoToProfile":true,
		"newPrice":null,
		"user":{
			"avatar": "https://public.onlyfans.com/files/m/mk/mka/mkamcrf6rjmcwo0jj4zoavhmalzohe5a1640180203/avatar.jpg",
			"id": 15585607,
			"name": "OnlyFans",
			"username": "onlyfans"
		}
	},
	"hasSystemNotifications": false
	}"#, socket::Message::NewMessage(_));

	socket_test!(test_stream_message, r#"{
	"stream": {
		"id": 2611175,
		"description": "stream description",
		"title": "stream title",
		"startedAt": "2022-11-05T14:02:24+00:00",
		"room": "dc2-room-roomId",
		"thumbUrl": "https://stream1-dc2.onlyfans.com/img/dc2-room-roomId/thumb.jpg",
		"user": {
			"avatar": "https://public.onlyfans.com/files/m/mk/mka/mkamcrf6rjmcwo0jj4zoavhmalzohe5a1640180203/avatar.jpg",
			"id": 15585607,
			"name": "OnlyFans",
			"username": "onlyfans"
		}
	}
}"#, socket::Message::Tagged(socket::TaggedMessage::Stream(_)));