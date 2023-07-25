use crate::structs::{ToToast, ContentType};
use serde::Deserialize;

#[derive(Deserialize, Debug)]
#[serde(default)]
pub struct Settings {
	pub notify: Whitelist,
	pub download: Whitelist,
	pub like: Whitelist,
	pub reconnect: bool
}

#[derive(Deserialize, Debug, Default)]
#[serde(default)]
pub struct GranularSelection {
	pub posts: GlobalSelection,
	pub messages: GlobalSelection,
	pub stories: GlobalSelection,
	pub streams: GlobalSelection,
	pub notifications: GlobalSelection
}

#[derive(Deserialize, Debug)]
#[serde(untagged)]
pub enum GlobalSelection {
	Select(Vec<String>),
	Full(bool),
}

#[derive(Deserialize, Debug)]
#[serde(untagged)]
pub enum Whitelist {
	Global(GlobalSelection),
	Granular(GranularSelection)
}

impl GlobalSelection {
	pub fn should_notify(&self, username: &str) -> bool {
		match &self {
			Self::Select(whitelist) => whitelist.iter().any(|s| s == username),
			Self::Full(b) => *b,
		}
	}

	pub fn should_download(&self, username: &str) -> bool {
		match &self {
			Self::Select(whitelist) => whitelist.iter().any(|s| s == username),
			Self::Full(b) => *b,
		}
	}

	pub fn should_like(&self, username: &str) -> bool {
		match &self {
			Self::Select(whitelist) => whitelist.iter().any(|s| s == username),
			Self::Full(b) => *b,
		}
	}
}

impl Settings {
	pub fn should_notify<C: ToToast>(&self, username: &str) -> bool {
		match &self.notify {
			Whitelist::Global(global) => global.should_notify(username),
			Whitelist::Granular(granular) => match C::header() {
				ContentType::Posts => granular.posts.should_notify(username),
				ContentType::Messages => granular.messages.should_notify(username),
				ContentType::Stories => granular.stories.should_notify(username),
				ContentType::Notifications => granular.notifications.should_notify(username),
				ContentType::Streams => granular.streams.should_notify(username)
			},
		}
	}

	pub fn should_download<C: ToToast>(&self, username: &str) -> bool {
		match &self.download {
			Whitelist::Global(global) => global.should_download(username),
			Whitelist::Granular(granular) => match C::header() {
				ContentType::Posts => granular.posts.should_download(username),
				ContentType::Messages => granular.messages.should_download(username),
				ContentType::Stories => granular.stories.should_download(username),
				_ => false
			},
		}
	}

	pub fn should_like<C: ToToast>(&self, username: &str) -> bool {
		match &self.like {
			Whitelist::Global(global) => global.should_like(username),
			Whitelist::Granular(granular) => match C::header() {
				ContentType::Posts => granular.posts.should_like(username),
				ContentType::Messages => granular.messages.should_like(username),
				ContentType::Stories => granular.stories.should_like(username),
				_ => false
			},
		}
	}
}

impl Default for Settings {
	fn default() -> Self {
		Settings {
			notify: Whitelist::Global(GlobalSelection::default()),
			download: Whitelist::Global(GlobalSelection::default()),
			like: Whitelist::Global(GlobalSelection::default()),
			reconnect: true
		}
	}
}

impl Default for GlobalSelection {
	fn default() -> Self {
		GlobalSelection::Full(false)
	}
}
