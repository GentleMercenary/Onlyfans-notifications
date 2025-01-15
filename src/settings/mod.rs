pub mod concrete;
pub mod actions;

use std::sync::{Arc, RwLock};

use concrete::{ConcreteSelection, MessageSpecificSelection, PostSpecificSelection, Toggle};
use log::LevelFilter;
use serde::Deserialize;
use actions::{Actions, ContentAction};

const fn default_log_level() -> LevelFilter {
	LevelFilter::Info
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct Settings {
	actions: Actions,
	pub reconnect: bool,
	#[serde(default = "default_log_level")]
	pub log_level: LevelFilter
}

impl Default for Settings {
	fn default() -> Self {
		Self {
			actions: Actions::default(),
			reconnect: true,
			log_level: default_log_level()
		}
	}
}

pub struct MediaContentActions<T> {
	pub notify: ConcreteSelection<T>,
	pub download: ConcreteSelection<T>,
	pub like: ConcreteSelection<T>,
}

pub struct StoryContentActions {
	pub notify: Toggle,
	pub download: Toggle,
	pub like: Toggle,
}

pub trait ResolveContentActions<T> {
	type Resolved;

	fn resolve(&self, data: &T) -> Self::Resolved;
}

mod private {
	pub trait Sealed {}
}

pub mod markers {
	pub struct PostMarker;
	pub struct MessageMarker;
	pub struct StoryMarker;
	pub struct StreamMarker;
	pub struct NotificationMarker;
}


impl private::Sealed for markers::PostMarker {}
impl private::Sealed for markers::MessageMarker {}
impl private::Sealed for markers::StoryMarker {}
impl private::Sealed for markers::StreamMarker {}
impl private::Sealed for markers::NotificationMarker {}

pub trait ContentActions<T: private::Sealed> {
	type Actions;
	fn content_actions(&self, username: &str) -> Self::Actions;
}

impl ContentActions<markers::PostMarker> for Settings {
	type Actions = MediaContentActions<PostSpecificSelection>;

	fn content_actions(&self, username: &str) -> Self::Actions {
		let actions = self.actions.get_actions_for(username);

		MediaContentActions {
			notify: match actions.notify {
				ContentAction::General(toggle) => ConcreteSelection::Toggle(toggle),
				ContentAction::Specific(specific) => specific.posts
			},
			download: match actions.download {
				ContentAction::General(toggle) => ConcreteSelection::Toggle(toggle),
				ContentAction::Specific(specific) => specific.posts
			},
			like: match actions.like {
				ContentAction::General(toggle) => ConcreteSelection::Toggle(toggle),
				ContentAction::Specific(specific) => specific.posts
			}
		}
	}
}

impl ContentActions<markers::MessageMarker> for Settings {
	type Actions = MediaContentActions<MessageSpecificSelection>;

	fn content_actions(&self, username: &str) -> Self::Actions {
		let actions = self.actions.get_actions_for(username);

		MediaContentActions {
			notify: match actions.notify {
				ContentAction::General(toggle) => ConcreteSelection::Toggle(toggle),
				ContentAction::Specific(specific) => specific.messages
			},
			download: match actions.download {
				ContentAction::General(toggle) => ConcreteSelection::Toggle(toggle),
				ContentAction::Specific(specific) => specific.messages
			},
			like: match actions.like {
				ContentAction::General(toggle) => ConcreteSelection::Toggle(toggle),
				ContentAction::Specific(specific) => specific.messages
			}
		}
	}
}

impl ContentActions<markers::StoryMarker> for Settings {
	type Actions = StoryContentActions;

	fn content_actions(&self, username: &str) -> Self::Actions {
		let actions = self.actions.get_actions_for(username);

		StoryContentActions {
			notify: match actions.notify {
				ContentAction::General(toggle) => toggle,
				ContentAction::Specific(specific) => specific.stories
			},
			download: match actions.download {
				ContentAction::General(toggle) => toggle,
				ContentAction::Specific(specific) => specific.stories
			},
			like: match actions.like {
				ContentAction::General(toggle) => toggle,
				ContentAction::Specific(specific) => specific.stories
			}
		}
	}
}

impl ContentActions<markers::StreamMarker> for Settings {
	type Actions = Toggle;

	fn content_actions(&self, username: &str) -> Self::Actions {
		let actions = self.actions.get_actions_for(username);
		match actions.notify {
			ContentAction::General(toggle) => toggle,
			ContentAction::Specific(specific) => specific.streams
		}
	}
}

impl ContentActions<markers::NotificationMarker> for Settings {
	type Actions = Toggle;

	fn content_actions(&self, username: &str) -> Self::Actions {
		let actions = self.actions.get_actions_for(username);
		match actions.notify {
			ContentAction::General(toggle) => toggle,
			ContentAction::Specific(specific) => specific.notifications
		}
	}
}

impl<T: private::Sealed> ContentActions<T> for Arc<RwLock<Settings>>
where Settings: ContentActions<T>
{
	type Actions = <Settings as ContentActions<T>>::Actions;

	fn content_actions(&self, username: &str) -> Self::Actions {
		self.read().unwrap()
		.content_actions(username)
	}
}