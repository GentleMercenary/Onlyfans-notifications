#![allow(dead_code)]

use crate::deserializers::{notification_message, from_string, from_string_vec, de_str_to_date_opt};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use of_client::{content, user::User, deserializers::de_str_to_date};

#[derive(Serialize, Debug)]
pub struct Connect<'a> {
	pub act: &'static str,
	pub token: &'a str,
}

#[derive(Serialize, Debug)]
pub struct Heartbeat<'a> {
	pub act: &'static str,
	pub ids: &'a [u64],
}

#[derive(Deserialize, Debug)]
pub struct Onlines {
	online: Vec<u64>
}

#[derive(Deserialize, Debug)]
pub struct Error {
	pub error: u8,
}

#[derive(Deserialize, Debug)]
pub struct Connected {
	connected: bool,
	v: String,
}

#[derive(Deserialize, Debug)]
pub struct PostPublished {
	#[serde(deserialize_with = "from_string")]
	pub id: u64,
	#[serde(deserialize_with="from_string")]
	user_id: u64,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct Fundraising {
	target: f32,
	target_progress: f32,
	#[serde(deserialize_with="from_string_vec")]
	presets: Vec<f32>
}
	
#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PostFundraisingUpdated {
	id: u64,
	fund_raising: Fundraising
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Chat {
	pub from_user: User,
	#[serde(flatten)]
	pub content: content::Chat,
}
	
#[derive(Deserialize, Debug)]
pub struct ChatCount {
	chat_messages: u32,
	count_priority_chat: u32,
	unread_tips: u32
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Story {
	pub user_id: u64,
	#[serde(flatten)]
	pub content: content::Story,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct ShortUser {
	pub id: u64,
	pub name: String,
	pub avatar: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct StoryTips {
	id: u64,
	from_user: ShortUser,
	story_user_id: u64,
	story_id: u64,
	amount: f32,
	message: String
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Notification {
	pub user: User,
	#[serde(rename = "type")]
	notif_type: String,
	sub_type: String,
	#[serde(flatten)]
	pub content: content::Notification,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Stream {
	pub user: User,
	#[serde(flatten)]
	pub content: content::Stream,
}

#[derive(Deserialize, Debug)]
pub struct StreamStart {
	#[serde(deserialize_with = "from_string")]
	stream_id: u64,
	#[serde(rename = "userId")]
	user_id: u64
}

#[derive(Deserialize, Debug)]
pub struct StreamStop {
	#[serde(deserialize_with = "from_string")]
	stream_id: u64,
	#[serde(deserialize_with = "from_string")]
	stream_user_id: u64
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct StreamUpdate {
	id: u64,
	raw_description: String,
	is_active: bool,
	is_finished: bool,
	#[serde(deserialize_with = "de_str_to_date")]
	started_at: DateTime<Utc>,
	#[serde(deserialize_with = "de_str_to_date_opt")]
	finished_at: Option<DateTime<Utc>>,
	room: String,
	likes_count: u32,
	views_count: u32,
	comments_count: u32,
	thumb_url: String,
	user: User,
	can_join: bool,
	partners: Vec<u64>,
	is_scheduled: bool,
	#[serde(deserialize_with = "de_str_to_date_opt")]
	scheduled_at: Option<DateTime<Utc>>,
	duration: u64,
	tips_goal: String,
}

#[derive(Deserialize, Debug)]
pub struct StreamLook {
	#[serde(deserialize_with = "from_string")]
	stream_user_id: u64,
	user: User,
	total: u32,
	viewer_instance_count: u32
}

#[derive(Deserialize, Debug)]
pub struct StreamComment {
	stream_user_id: u64,
	comment_id: u64,
	comment: String,
	user: User
}

#[derive(Deserialize, Debug)]
pub struct StreamLike {
	#[serde(deserialize_with = "from_string")]
	stream_user_id: u64
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum TaggedMessage {
	PostPublished(PostPublished),
	#[serde(deserialize_with = "from_string")]
	PostUpdated(u64),
	#[serde(deserialize_with = "from_string")]
	PostExpire(u64),
	PostFundraisingUpdated(PostFundraisingUpdated),

	Api2ChatMessage(Chat),

	Stories(Vec<Story>),
	StoryTips(StoryTips),

	Stream(Stream),
	StreamStart(StreamStart),
	StreamStop(StreamStop),
	StreamUpdate(StreamUpdate),
	StreamLook(StreamLook),
	StreamUnlook(StreamLook),
	StreamComment(StreamComment),
	StreamLike(StreamLike)
}

#[derive(Deserialize, Debug)]
#[serde(untagged)]
pub enum Message {
	Tagged(TaggedMessage),
	Onlines(Onlines),
	ChatCount(ChatCount),
	Connected(Connected),
	#[serde(deserialize_with = "notification_message")]
	#[allow(clippy::enum_variant_names)]
	NewMessage(Notification),
	Error(Error),
}