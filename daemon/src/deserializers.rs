use std::{fmt::Display, marker::PhantomData, str::FromStr};
use serde::{de::{Error, SeqAccess, Visitor}, Deserialize, Deserializer};

pub fn from<'de, D, T, U>(deserializer: D) -> Result<U, D::Error>
where
	D: Deserializer<'de>,
	T: Deserialize<'de>,
	U: From<T>
{
	T::deserialize(deserializer).map(Into::into)
}

pub fn from_str<'de, T, D>(deserializer: D) -> Result<T, D::Error>
where
	D: Deserializer<'de>,
	T: FromStr,
	<T as FromStr>::Err: Display,
{
	Deserialize::deserialize(deserializer)
	.and_then(|s: &str| T::from_str(s).map_err(Error::custom))
}

pub fn from_str_seq<'de, T, D>(deserializer: D) -> Result<Vec<T>, D::Error>
where
	D: Deserializer<'de>,
	T: FromStr,
	<T as FromStr>::Err: Display,
{

	struct SeqVisitor<T>(PhantomData<T>);

	impl<'de, T> Visitor<'de> for SeqVisitor<T>
	where
		T: FromStr,
		<T as FromStr>::Err: Display,
	{
		type Value = Vec<T>;

		fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
			formatter.write_str("a sequence of things that implement FromStr")
		}

		fn visit_seq<S>(self, mut seq: S) -> Result<Self::Value, S::Error>
		where
			S: SeqAccess<'de>
		{
			let mut vec: Vec<T> = match seq.size_hint() {
				Some(size) => Vec::with_capacity(size),
				None => Vec::new()
			};

			while let Some(s) = seq.next_element::<&str>()? {
				let val = T::from_str(s).map_err(Error::custom)?;
				vec.push(val);
			}

			Ok(vec)
		}
	}

	let visitor = SeqVisitor(PhantomData);
	deserializer.deserialize_seq(visitor)

}