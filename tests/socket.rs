mod init;

use of_notifier::{get_auth_params, structs::socket, init, SETTINGS, settings::{Settings, Selection, CoarseSelection}};
use of_client::client::OFClient;
use std::thread::sleep;
use std::time::Duration;
use std::sync::Once;
use tokio::sync::RwLock;
use init::init_log;

static INIT: Once = Once::new();

fn test_init() {
	INIT.call_once(|| {
		init().unwrap();

		SETTINGS
		.set(RwLock::new(Settings {
			notify: Selection::Coarse(CoarseSelection::from(true)),
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
			let client = OFClient::new(params).await.unwrap();
			msg.handle_message(&client).await.unwrap();
			sleep(Duration::from_millis(1000));
		}
	};
}

socket_test!(test_post_message,  r#"{
	"post_published": {
		"id": "492747400",
		"user_id" : "15585607",
		"show_posts_in_feed":true
	}
}"#, socket::Message::Tagged(socket::TaggedMessage::PostPublished(_)));

socket_test!(test_post_updated_message, r#"{"post_updated": "492747400"}"#, socket::Message::Tagged(socket::TaggedMessage::PostUpdated(_)));

socket_test!(test_post_expire_message, r#"{"post_expire": "492747400"}"#, socket::Message::Tagged(socket::TaggedMessage::PostExpire(_)));

socket_test!(test_post_fundraising_message, r#"{
	"post_fundraising_updated": {
		"id": 1234,
		"fundRaising": {
			"target": 123.99,
			"targetProgress": 39.99,
			"presets": ["10","20","50","100"]
		}
	}
}"#, socket::Message::Tagged(socket::TaggedMessage::PostFundraisingUpdated(_)));


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
				"files": {
					"full": {
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
}"#, socket::Message::Tagged(socket::TaggedMessage::Api2ChatMessage(_)));

socket_test!(test_story_message, r#"{
	"stories": [
		{
			"id": 0,
			"userId": 15585607,
			"canLike": false,
			"media":[
				{
					"id": 0,
					"canView": true,
					"files": {
						"full": {
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

socket_test!(test_story_tips_message, r#"{
	"story_tips": {
		"id": 123,
		"from_user": {
			"id": 15585607,
			"name": "OnlyFans"
		},
		"story_user_id": 15585607,
		"story_id": 456,
		"amount": 10,
		"amount_human": "$10.00",
		"message": "Test tip"
	}
}"#, socket::Message::Tagged(socket::TaggedMessage::StoryTips(_)));

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
		"id": 0,
		"description": "stream description",
		"title": "stream title",
		"startedAt": "2022-11-05T14:02:24+00:00",
		"room": "dc2-room-roomId",
		"thumbUrl": "https://raw.githubusercontent.com/allenbenz/winrt-notification/main/resources/test/chick.jpeg",
		"user": {
			"avatar": "https://public.onlyfans.com/files/m/mk/mka/mkamcrf6rjmcwo0jj4zoavhmalzohe5a1640180203/avatar.jpg",
			"id": 15585607,
			"name": "OnlyFans",
			"username": "onlyfans"
		}
	}
}"#, socket::Message::Tagged(socket::TaggedMessage::Stream(_)));

socket_test!(test_stream_start_message, r#"{
	"stream_start": {
		"stream_id": "1234",
		"userId": 15585607
	}
}"#, socket::Message::Tagged(socket::TaggedMessage::StreamStart(_)));

socket_test!(test_stream_stop_message, r#"{
	"stream_stop":{
	"stream_id": "1234",
	"stream_user_id": "15585607"
	}
}"#, socket::Message::Tagged(socket::TaggedMessage::StreamStop(_)));

socket_test!(test_stream_update_message, r#"{
	"stream_update": {
		"id": 1234,
		"description": "stream description",
		"rawDescription": "stream description",
		"isActive": true,
		"isFinished": false,
		"startedAt": "1970-01-01T00:00:00+00:00",
		"finishedAt": null,
		"room": "channel_1234",
		"streamingPlatform": "gateway",
		"likesCount": 101,
		"viewsCount": 202,
		"commentsCount": 303,
		"thumbUrl": "https://raw.githubusercontent.com/allenbenz/winrt-notification/main/resources/test/chick.jpeg",
		"user": {
			"avatar": "https://public.onlyfans.com/files/m/mk/mka/mkamcrf6rjmcwo0jj4zoavhmalzohe5a1640180203/avatar.jpg",
			"id": 15585607,
			"name": "OnlyFans",
			"username": "onlyfans"
		},
		"canJoin": false,
		"partners": [],
		"isScheduled": false,
		"scheduledAt": null,
		"duration": 0,
		"tipsGoal": "stream tip goal"
	}
}"#, socket::Message::Tagged(socket::TaggedMessage::StreamUpdate(_)));

socket_test!(test_stream_look_message, r#"{
	"stream_look": {
		"stream_user_id": "15585607",
		"user": {
			"avatar": "https://public.onlyfans.com/files/m/mk/mka/mkamcrf6rjmcwo0jj4zoavhmalzohe5a1640180203/avatar.jpg",
			"id": 15585607,
			"name": "OnlyFans",
			"username": "onlyfans"
		},
		"total": 9001,
		"viewer_instance_count": 42
	}
}"#, socket::Message::Tagged(socket::TaggedMessage::StreamLook(_)));

socket_test!(test_stream_unlook_message, r#"{
	"stream_unlook": {
		"stream_user_id": "15585607",
		"total": 9002,
		"viewer_instance_count": 43,
		"is_user_blocked": false,
		"user": {
			"avatar": "https://public.onlyfans.com/files/m/mk/mka/mkamcrf6rjmcwo0jj4zoavhmalzohe5a1640180203/avatar.jpg",
			"id": 15585607,
			"name": "OnlyFans",
			"username": "onlyfans"
		}
	}
}"#, socket::Message::Tagged(socket::TaggedMessage::StreamUnlook(_)));

socket_test!(test_stream_comment_message, r#"{
	"stream_comment": {
		"stream_user_id": 15585607,
		"comment_id": 1234,
		"comment": "comment text",
		"isPrivate": false,
		"user": {
			"avatar": "https://public.onlyfans.com/files/m/mk/mka/mkamcrf6rjmcwo0jj4zoavhmalzohe5a1640180203/avatar.jpg",
			"id": 15585607,
			"name": "OnlyFans",
			"username": "onlyfans"
		}
	}
}"#, socket::Message::Tagged(socket::TaggedMessage::StreamComment(_)));

socket_test!(test_stream_like_message, r#"{
	"stream_like": {
		"stream_user_id": "15585607",
		"x": 0,
		"y": 0
	}
}"#, socket::Message::Tagged(socket::TaggedMessage::StreamLike(_)));

socket_test!(test_chat_count_message, r#"{
	"chat_messages": 3,
	"count_priority_chat": 2,
	"unread_tips": 1
}"#, socket::Message::ChatCount(_));