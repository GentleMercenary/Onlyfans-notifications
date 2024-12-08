use std::collections::HashSet;
use log::LevelFilter;
use of_client::content::ContentType;
use crate::deserializers::de_log_level;
use serde::Deserialize;

const fn default_log_level() -> LevelFilter {
	LevelFilter::Info
}

#[derive(Deserialize, Debug)]
#[serde(untagged)]
pub enum CoarseSelection {
	Whitelist(HashSet<String>),
	All(bool),
}

#[derive(Deserialize, Debug, Default)]
#[serde(default)]
pub struct GranularSelection {
	posts: CoarseSelection,
	messages: CoarseSelection,
	stories: CoarseSelection,
	streams: CoarseSelection,
	notifications: CoarseSelection
}

#[derive(Deserialize, Debug)]
#[serde(untagged)]
pub enum Selection {
	Coarse(CoarseSelection),
	Granular(GranularSelection)
}

#[derive(Deserialize, Debug)]
#[serde(default)]
pub struct Settings {
	pub notify: Selection,
	pub download: Selection,
	pub like: Selection,
	pub reconnect: bool,
	#[serde(default = "default_log_level")]
	#[serde(deserialize_with="de_log_level")]
	pub log_level: LevelFilter
}

impl CoarseSelection {
	pub fn enabled_for(&self, username: &str) -> bool {
		match &self {
			Self::Whitelist(whitelist) => whitelist.contains(username),
			Self::All(b) => *b,
		}
	}
}

impl From<bool> for CoarseSelection {
	fn from(value: bool) -> Self {
		Self::All(value)
	}
}

impl Default for CoarseSelection {
	fn default() -> Self {
		Self::from(false)
	}
}

impl Selection {
	pub fn enabled_for(&self, username: &str, content_type: ContentType) -> bool {
		match self {
			Self::Coarse(coarse) => coarse.enabled_for(username),
			Self::Granular(granular) => match content_type {
				ContentType::Posts => granular.posts.enabled_for(username),
				ContentType::Chats => granular.messages.enabled_for(username),
				ContentType::Stories => granular.stories.enabled_for(username),
				ContentType::Notifications => granular.notifications.enabled_for(username),
				ContentType::Streams => granular.streams.enabled_for(username)
			},
		}
	}
}

impl Default for Settings {
	fn default() -> Self {
		Self {
			notify: Selection::Coarse(CoarseSelection::default()),
			download: Selection::Coarse(CoarseSelection::default()),
			like: Selection::Coarse(CoarseSelection::default()),
			reconnect: true,
			log_level: default_log_level()
		}
	}
}