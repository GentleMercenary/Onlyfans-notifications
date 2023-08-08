use std::collections::HashSet;

use of_client::content::{ContentType, self};
use serde::Deserialize;

#[derive(Deserialize, Debug, Clone)]
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
	pub reconnect: bool
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
		CoarseSelection::All(value)
	}
}

impl Default for CoarseSelection {
	fn default() -> Self {
		CoarseSelection::from(false)
	}
}

impl Selection {
	pub fn enabled_for(&self, username: &str, content_type: ContentType) -> bool {
		match self {
			Selection::Coarse(coarse) => coarse.enabled_for(username),
			Selection::Granular(granular) => match content_type {
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
		Settings {
			notify: Selection::Coarse(CoarseSelection::default()),
			download: Selection::Coarse(CoarseSelection::default()),
			like: Selection::Coarse(CoarseSelection::default()),
			reconnect: true
		}
	}
}

pub trait ShouldNotify {
	fn should_notify(&self, username: &str, settings: &Settings) -> bool;
}

pub trait ShouldDownload {
	fn should_download(&self, username: &str, settings: &Settings) -> bool;
}

pub trait ShouldLike {
	fn should_like(&self, username: &str, settings: &Settings) -> bool;
}

impl<T: content::Content> ShouldNotify for T {
	fn should_notify(&self, username: &str, settings: &Settings) -> bool {
		settings.notify.enabled_for(username, T::content_type())
	}
}

impl<T: content::HasMedia> ShouldDownload for T {
	fn should_download(&self, username: &str, settings: &Settings) -> bool {
		settings.download.enabled_for(username, T::content_type())
	}
}

impl<T: content::CanLike> ShouldLike for T {
	fn should_like(&self, username: &str, settings: &Settings) -> bool {
		settings.like.enabled_for(username, T::content_type())
	}
}