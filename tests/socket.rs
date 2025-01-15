mod init;

use of_notifier::{handlers::{Handler, Context}, init_cdm, init_client, settings::Settings};
use of_daemon::structs::{Message, TaggedMessage};
use std::{sync::{OnceLock, Arc, RwLock}, thread::sleep, time::Duration};
use init::init_log;

static HANDLER: OnceLock<Context> = OnceLock::new();

macro_rules! socket_test {
	($name: ident, $incoming: expr, $match: pat) => {
		#[tokio::test]
		async fn $name() {
			init_log();

			let msg = serde_json::from_str::<Message>($incoming).unwrap();
			assert!(matches!(msg, $match));
	
			let context = HANDLER.get_or_init(|| {
				let settings = Settings::default();
				let client = init_client().unwrap();
				let cdm = init_cdm().ok();
				Context::new(client, cdm, Arc::new(RwLock::new(settings))).unwrap()
			});

			if let Some(handle) = msg.handle(context).unwrap() {
				let _ = handle.await;
				sleep(Duration::from_millis(100));
			}
		}
	};
}

socket_test!(test_post_message, r#"{
	"post_published": {
		"id": "492747400",
		"user_id" : "15585607",
		"show_posts_in_feed":true
	}
}"#, Message::Tagged(TaggedMessage::PostPublished(_)));

socket_test!(test_post_updated_message, r#"{"post_updated": "492747400"}"#, Message::Tagged(TaggedMessage::PostUpdated(_)));

socket_test!(test_post_expire_message, r#"{"post_expire": "492747400"}"#, Message::Tagged(TaggedMessage::PostExpire(_)));

socket_test!(test_post_fundraising_message, r#"{
	"post_fundraising_updated": {
		"id": 1234,
		"fundRaising": {
			"target": 123.99,
			"targetProgress": 39.99,
			"presets": ["10","20","50","100"]
		}
	}
}"#, Message::Tagged(TaggedMessage::PostFundraisingUpdated(_)));

socket_test!(test_chat_message, r#"{
	"api2_chat_message": {
		"id": 0,
		"text": "<p>This is a message</p><p><br />testing <a href = \"/onlyfans\">MARKDOWN parsing</a> 👌<br />\n in notifications 💯</p>",
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
}"#, Message::Tagged(TaggedMessage::Api2ChatMessage(_)));

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
}"#, Message::Tagged(TaggedMessage::Stories(_)));

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
}"#, Message::Tagged(TaggedMessage::StoryTips(_)));

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
}"#, Message::Notification(_));

socket_test!(test_notification_count_message, r#"{"messages":1,"hasSystemNotifications":false}"#, Message::NotificationCount(_));

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
}"#, Message::Tagged(TaggedMessage::Stream(_)));

socket_test!(test_stream_start_message, r#"{
	"stream_start": {
		"stream_id": "1234",
		"userId": 15585607
	}
}"#, Message::Tagged(TaggedMessage::StreamStart(_)));

socket_test!(test_stream_stop_message, r#"{
	"stream_stop":{
	"stream_id": "1234",
	"stream_user_id": "15585607"
	}
}"#, Message::Tagged(TaggedMessage::StreamStop(_)));

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
}"#, Message::Tagged(TaggedMessage::StreamUpdate(_)));

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
}"#, Message::Tagged(TaggedMessage::StreamLook(_)));

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
}"#, Message::Tagged(TaggedMessage::StreamUnlook(_)));

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
}"#, Message::Tagged(TaggedMessage::StreamComment(_)));

socket_test!(test_stream_like_message, r#"{
	"stream_like": {
		"stream_user_id": "15585607",
		"x": 0,
		"y": 0
	}
}"#, Message::Tagged(TaggedMessage::StreamLike(_)));

socket_test!(test_stream_tips_message, r#"{
	"stream_tips": {
		"id": 1234,
		"from_user": {
			"avatar": "https://public.onlyfans.com/files/m/mk/mka/mkamcrf6rjmcwo0jj4zoavhmalzohe5a1640180203/avatar.jpg",
			"id": 15585607,
			"name": "OnlyFans",
			"username": "onlyfans"
		},
		"stream_user_id": 15585607,
		"stream_id": 5678,
		"amount": 5,
		"amount_human": "$5.00",
		"message": null
	},
	"is_show_tips": true,
	"tips_count": 5,
	"tips_summ": 24.5,
	"is_show_tips_goal": true,
	"tips_goal": "$100 Tip goal \u2665",
	"tips_goal_sum": 100,
	"tips_goal_progress": 24.5
}"#, Message::StreamTips(_));



socket_test!(test_chat_count_message, r#"{
	"chat_messages": 3,
	"count_priority_chat": 2,
	"unread_tips": 1
}"#, Message::ChatCount(_));

socket_test!(test_new_hints_message, r#"{"has_new_hints":true}"#, Message::Tagged(TaggedMessage::HasNewHints(_)));