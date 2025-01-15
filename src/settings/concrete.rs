use std::{marker::PhantomData, ops::Deref, str::FromStr};
use serde::{de::{self, Visitor}, Deserialize, Deserializer};
use thiserror::Error;

#[derive(Debug, Clone, Copy)]
pub struct Toggle(pub bool);

#[derive(Error, Debug)]
#[error("Unknown variant")]
pub struct UnknownVariant;

impl FromStr for Toggle {
	type Err = UnknownVariant;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		if s.eq_ignore_ascii_case("all") { Ok(Toggle(true)) }
		else if s.eq_ignore_ascii_case("none") { Ok(Toggle(false)) }
		else { Err(UnknownVariant) }
	}
}

impl<'de> Deserialize<'de> for Toggle {
	fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
		struct ToggleVisitor;
		impl Visitor<'_> for ToggleVisitor {
			type Value = Toggle;
	
			fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
				formatter.write_str("a boolean, \"all\", or \"none\"")
			}
	
			fn visit_bool<E: de::Error>(self, v: bool) -> Result<Self::Value, E> {
				Ok(Toggle(v))
			}
	
			fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
				v.parse::<Toggle>()
				.map_err(|_| de::Error::unknown_variant(v, &["all", "none"]))
			}
		}
	
		deserializer.deserialize_any(ToggleVisitor)
	}
}

impl Deref for Toggle {
	type Target = bool;
	fn deref(&self) -> &Self::Target { &self.0 }
}

#[derive(Debug, Clone)]
pub enum ConcreteSelection<T> {
	Toggle(Toggle),
	Specific(T)
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for ConcreteSelection<T> {
	fn deserialize<D: Deserializer<'de>,>(deserializer: D) -> Result<Self, D::Error> {
		struct ConcreteSelectionVisitor<T>(PhantomData<T>);
		impl<'de, T: Deserialize<'de>> Visitor<'de> for ConcreteSelectionVisitor<T> {
			type Value = ConcreteSelection<T>;

			fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
				formatter.write_str("a boolean, \"all\", \"none\", or a type-specific struct")
			}

			fn visit_bool<E: de::Error>(self, v: bool) -> Result<Self::Value, E> {
				Ok(ConcreteSelection::Toggle(Toggle(v)))
			}

			fn visit_map<A: de::MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
				Deserialize::deserialize(de::value::MapAccessDeserializer::new(map))
				.map(ConcreteSelection::Specific)
			}
		}

		deserializer.deserialize_any(ConcreteSelectionVisitor(PhantomData))
	}
}

#[derive(Debug, Clone, Copy)]
pub enum MediaSelection {
	Any,
	Thumbnail,
	None
}

impl FromStr for MediaSelection {
	type Err = UnknownVariant;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		if s.eq_ignore_ascii_case("any") { Ok(MediaSelection::Any) }
		else if s.eq_ignore_ascii_case("none") { Ok(MediaSelection::None) }
		else if s.eq_ignore_ascii_case("thumbnail") { Ok(MediaSelection::Thumbnail) }
		else { Err(UnknownVariant) }
	}
}

impl<'de> Deserialize<'de> for MediaSelection {
	fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
		Deserialize::deserialize(deserializer)
		.and_then(|s: &str| MediaSelection::from_str(s).map_err(|_| de::Error::unknown_variant(s, &["any", "none", "thumbnail"])))
	}
}

#[derive(Deserialize, Debug, Clone, Copy)]
pub struct ConcreteMediaSpecificSelection {
	pub media: MediaSelection
}

pub type PostSpecificSelection = ConcreteMediaSpecificSelection;
pub type MessageSpecificSelection = ConcreteMediaSpecificSelection;