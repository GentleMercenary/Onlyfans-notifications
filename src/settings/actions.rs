use std::collections::{HashMap, HashSet};
use serde::{Deserialize, Deserializer};
use super::selection::{Selection, Toggle, MessageSpecificSelection, PostSpecificSelection};

trait Merge<T> {
	fn merge(&self, base: &T) -> T;
}

impl<X, Y> Merge<Selection<X>> for Selection<Y>
where
	Y: Merge<X>,
	X: Clone + From<Toggle>
{
	fn merge(&self, base: &Selection<X>) -> Selection<X> {
		match self {
			Selection::General(general) => Selection::General(*general),
			Selection::Specific(specific) => Selection::Specific(
				match base {
					Selection::General(base_general) => specific.merge(&X::from(*base_general)),
					Selection::Specific(base_specific) => specific.merge(base_specific)
				}
			)
		}
	}
}

impl<X> Merge<Option<X>> for Option<X>
where
	X: Merge<X> + Clone
{
	fn merge(&self, base: &Option<X>) -> Option<X> {
		match (self, base) {
			(None, None) => None,
			(Some(a), None) => Some(a.clone()),
			(None, Some(b)) => Some(b.clone()),
			(Some(a), Some(b)) => Some(a.merge(b))
		}
	}
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct AllContent {
	pub posts: Selection<PostSpecificSelection>,
	pub messages: Selection<MessageSpecificSelection>,
	pub stories: Toggle,
	pub streams: Toggle,
	pub notifications: Toggle
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct PartialAllContent {
	posts: Option<Selection<PostSpecificSelection>>,
	messages: Option<Selection<MessageSpecificSelection>>,
	stories: Option<Toggle>,
	streams: Option<Toggle>,
	notifications: Option<Toggle>
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct MediaContent {
	pub posts: Selection<PostSpecificSelection>,
	pub messages: Selection<MessageSpecificSelection>,
	pub stories: Toggle,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct PartialMediaContent {
	posts: Option<Selection<PostSpecificSelection>>,
	messages: Option<Selection<MessageSpecificSelection>>,
	stories: Option<Toggle>,
}

impl Merge<AllContent> for PartialAllContent {
	fn merge(&self, base: &AllContent) -> AllContent {
		AllContent {
			posts: self.posts.as_ref().unwrap_or(&base.posts).clone(),
			messages: self.messages.as_ref().unwrap_or(&base.messages).clone(),
			stories: self.stories.unwrap_or(base.stories),
			streams: self.streams.unwrap_or(base.streams),
			notifications: self.notifications.unwrap_or(base.notifications)
		}
	}
}

impl Merge<PartialAllContent> for PartialAllContent {
	fn merge(&self, base: &PartialAllContent) -> PartialAllContent {
		PartialAllContent {
			posts: self.posts.as_ref().or(base.posts.as_ref()).cloned(),
			messages: self.messages.as_ref().or(base.messages.as_ref()).cloned(),
			stories: self.stories.or(base.stories),
			streams: self.streams.or(base.streams),
			notifications: self.notifications.or(base.notifications)
		}
	}
}

impl Merge<MediaContent> for PartialMediaContent {
	fn merge(&self, base: &MediaContent) -> MediaContent {
		MediaContent {
			posts: self.posts.as_ref().unwrap_or(&base.posts).clone(),
			messages: self.messages.as_ref().unwrap_or(&base.messages).clone(),
			stories: self.stories.unwrap_or(base.stories)
		}
	}
}

impl Merge<PartialMediaContent> for PartialMediaContent {
	fn merge(&self, base: &PartialMediaContent) -> PartialMediaContent {
		PartialMediaContent {
			posts: self.posts.as_ref().or(base.posts.as_ref()).cloned(),
			messages: self.messages.as_ref().or(base.messages.as_ref()).cloned(),
			stories: self.stories.or(base.stories)
		}
	}
}

impl From<Toggle> for AllContent {
	fn from(value: Toggle) -> Self {
		Self {
			posts: Selection::General(value),
			messages: Selection::General(value),
			stories: value,
			streams: value,
			notifications: value
		}
	}
}

impl From<Toggle> for PartialAllContent {
	fn from(value: Toggle) -> Self {
		Self {
			posts: Some(Selection::General(value)),
			messages: Some(Selection::General(value)),
			stories: Some(value),
			streams: Some(value),
			notifications: Some(value)
		}
	}
}

impl From<Toggle> for MediaContent {
	fn from(value: Toggle) -> Self {
		Self {
			posts: Selection::General(value),
			messages: Selection::General(value),
			stories: value
		}
	}
}

impl From<Toggle> for PartialMediaContent {
	fn from(value: Toggle) -> Self {
		Self {
			posts: Some(Selection::General(value)),
			messages: Some(Selection::General(value)),
			stories: Some(value)
		}
	}
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct DefaultActions {
	pub notify: Selection<AllContent>,
	pub download: Selection<MediaContent>,
	pub like: Selection<MediaContent>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct ExceptionActions {
	notify: Option<Selection<PartialAllContent>>,
	download: Option<Selection<PartialMediaContent>>,
	like: Option<Selection<PartialMediaContent>>
}

impl Merge<ExceptionActions> for ExceptionActions {
	fn merge(&self, base: &ExceptionActions) -> ExceptionActions {
		ExceptionActions {
			notify: self.notify.merge(&base.notify),
			download: self.download.merge(&base.download),
			like: self.like.merge(&base.like)
		}
	}
}

impl Merge<DefaultActions> for ExceptionActions {
	fn merge(&self, base: &DefaultActions) -> DefaultActions {
		DefaultActions {
			notify: self.notify.as_ref().map_or_else(|| base.notify.clone(), |action| action.merge(&base.notify)),
			download: self.download.as_ref().map_or_else(|| base.download.clone(), |action| action.merge(&base.download)),
			like: self.like.as_ref().map_or_else(|| base.like.clone(), |action| action.merge(&base.like))
		}
	}
}

fn exceptions<'de, D: Deserializer<'de>>(deserializer: D) -> Result<HashMap<String, ExceptionActions>, D::Error> {
	#[derive(Deserialize, Debug)]
	#[serde(deny_unknown_fields)]
	struct Exception {
		users: HashSet<String>,
		actions: ExceptionActions
	}

	let exceptions: Vec<Exception> = Deserialize::deserialize(deserializer)?;
	let mut res: HashMap<String, ExceptionActions> = HashMap::new();
	for exception in exceptions {
		for user in exception.users {
			res.entry(user)
			.and_modify(|exisiting| *exisiting = exisiting.merge(&exception.actions))
			.or_insert_with(|| exception.actions.clone());
		}
	}

	Ok(res)
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct Actions {
	pub default: DefaultActions,
	#[serde(deserialize_with = "exceptions")]
	exceptions: HashMap<String, ExceptionActions>
}

impl Actions {
	pub fn get_actions_for(&self, username: &str) -> DefaultActions {
		self.exceptions
		.get(username)
		.map_or_else(|| self.default.clone(), |exception| exception.merge(&self.default))
	}
}

impl Default for Actions {
	fn default() -> Self {
		Self {
			default: DefaultActions {
				notify: Selection::General(Toggle(true)),
				download: Selection::General(Toggle(true)),
				like: Selection::General(Toggle(false)),
			},
			exceptions: HashMap::new()
		}
	}
}