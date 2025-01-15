use std::{collections::{HashMap, HashSet}, marker::PhantomData};
use serde::{de::{self, Visitor}, Deserialize, Deserializer};
use crate::settings::concrete::{ConcreteSelection, Toggle};

use super::concrete::{MessageSpecificSelection, PostSpecificSelection};

#[derive(Debug, Clone)]
pub enum ContentAction<T> {
	General(Toggle),
	Specific(T)
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for ContentAction<T> {
	fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
		struct GeneralOrSpecific<T>(PhantomData<T>);
		impl<'de, T: Deserialize<'de>> Visitor<'de> for GeneralOrSpecific<T> {
			type Value = ContentAction<T>;

			fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
				formatter.write_str("A boolean, \"all\", \"none\" or a map defining a selection per content type")
			}

			fn visit_bool<E: de::Error>(self, v: bool) -> Result<Self::Value, E> {
				Ok(ContentAction::General(Toggle(v)))
			}

			fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
				v.parse::<Toggle>()
				.map(|toggle| ContentAction::General(toggle))
				.map_err(|_| de::Error::unknown_variant(v, &["all", "none"]))
			}

			fn visit_map<A: de::MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
				Deserialize::deserialize(de::value::MapAccessDeserializer::new(map))
				.map(ContentAction::Specific)
			}
		}

		deserializer.deserialize_any(GeneralOrSpecific(PhantomData))
	}
}

trait Merge<T> {
	fn merge(&self, base: &T) -> T;
}

impl<X, Y> Merge<ContentAction<X>> for ContentAction<Y>
where
	Y: Merge<X>,
	X: Clone + From<Toggle>
{
	fn merge(&self, base: &ContentAction<X>) -> ContentAction<X> {
		match self {
			ContentAction::General(general) => ContentAction::General(*general),
			ContentAction::Specific(specific) => ContentAction::Specific(
				match base {
					ContentAction::General(base_general) => specific.merge(&X::from(*base_general)),
					ContentAction::Specific(base_specific) => specific.merge(base_specific)
				}
			)
		}
	}
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct AllContent {
	pub posts: ConcreteSelection<PostSpecificSelection>,
	pub messages: ConcreteSelection<MessageSpecificSelection>,
	pub stories: Toggle,
	pub streams: Toggle,
	pub notifications: Toggle
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct PartialAllContent {
	posts: Option<ConcreteSelection<PostSpecificSelection>>,
	messages: Option<ConcreteSelection<MessageSpecificSelection>>,
	stories: Option<Toggle>,
	streams: Option<Toggle>,
	notifications: Option<Toggle>
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct MediaContent {
	pub posts: ConcreteSelection<PostSpecificSelection>,
	pub messages: ConcreteSelection<MessageSpecificSelection>,
	pub stories: Toggle,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct PartialMediaContent {
	posts: Option<ConcreteSelection<PostSpecificSelection>>,
	messages: Option<ConcreteSelection<MessageSpecificSelection>>,
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
			posts: ConcreteSelection::Toggle(value),
			messages: ConcreteSelection::Toggle(value),
			stories: value,
			streams: value,
			notifications: value
		}
	}
}

impl From<Toggle> for PartialAllContent {
	fn from(value: Toggle) -> Self {
		Self {
			posts: Some(ConcreteSelection::Toggle(value)),
			messages: Some(ConcreteSelection::Toggle(value)),
			stories: Some(value),
			streams: Some(value),
			notifications: Some(value)
		}
	}
}

impl From<Toggle> for MediaContent {
	fn from(value: Toggle) -> Self {
		Self {
			posts: ConcreteSelection::Toggle(value),
			messages: ConcreteSelection::Toggle(value),
			stories: value
		}
	}
}

impl From<Toggle> for PartialMediaContent {
	fn from(value: Toggle) -> Self {
		Self {
			posts: Some(ConcreteSelection::Toggle(value)),
			messages: Some(ConcreteSelection::Toggle(value)),
			stories: Some(value)
		}
	}
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct DefaultActions {
	pub notify: ContentAction<AllContent>,
	pub download: ContentAction<MediaContent>,
	pub like: ContentAction<MediaContent>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct ExceptionActions {
	notify: Option<ContentAction<PartialAllContent>>,
	download: Option<ContentAction<PartialMediaContent>>,
	like: Option<ContentAction<PartialMediaContent>>
}

impl Merge<ExceptionActions> for ExceptionActions {
	fn merge(&self, base: &ExceptionActions) -> ExceptionActions {
		ExceptionActions {
			notify: self.notify.as_ref().zip_with(base.notify.as_ref(), |existing, incoming| existing.merge(incoming)),
			download: self.download.as_ref().zip_with(base.download.as_ref(), |existing, incoming| existing.merge(incoming)),
			like: self.like.as_ref().zip_with(base.like.as_ref(), |existing, incoming| existing.merge(incoming))
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
				notify: ContentAction::General(Toggle(true)),
				download: ContentAction::General(Toggle(true)),
				like: ContentAction::General(Toggle(false)),
			},
			exceptions: HashMap::new()
		}
	}
}