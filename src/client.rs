use crate::deserializers::{parse_cookie, non_empty_string};
use crate::structs::content::{StoryContent, MessageContent};
use crate::structs::{User, content::PostContent};

use serde::Deserialize;
use cached::proc_macro::once;
use anyhow::{anyhow, Context};
use futures::{StreamExt, TryFutureExt};
use crypto::{digest::Digest, sha1::Sha1};
use tokio_retry::{strategy::FixedInterval, Retry};
use reqwest::{cookie::Jar, header, Client, Response, Url};
use std::{fmt, fs::{self, File}, io::Write, path::{Path, PathBuf}, sync::Arc, time::{SystemTime, UNIX_EPOCH}};

#[derive(Debug)]
pub struct Cookie {
	pub auth_id: String,
	pub sess: String,
	pub auth_hash: String,
}

#[derive(Deserialize, Debug)]
pub struct AuthParams {
	#[serde(deserialize_with = "parse_cookie")]
	cookie: Cookie,
	#[serde(deserialize_with = "non_empty_string")]
	x_bc: String,
	#[serde(deserialize_with = "non_empty_string")]
	user_agent: String,
}

#[derive(Deserialize, Debug, Clone)]
struct StaticParams {
	static_param: String,
	format: String,
	checksum_indexes: Vec<i32>,
	checksum_constant: i32,
	app_token: String,
	remove_headers: Vec<String>,
}

#[once(time = 1800, result = true, sync_writes = true)]
async fn get_static_params() -> anyhow::Result<StaticParams> {
	reqwest::get("https://raw.githubusercontent.com/DIGITALCRIMINALS/dynamic-rules/main/onlyfans.json")
	.inspect_err(|err| error!("Error getting dynamic rules: {err:?}"))
	.and_then(Response::json::<StaticParams>)
	.await
	.inspect_err(|err| error!("Error reading dynamic rules: {err:?}"))
	.map_err(Into::into)
}

pub struct Unauthorized;
#[derive(Debug)]
pub struct Authorized { client: Client, params: AuthParams }

#[derive(Debug)]
pub struct OFClient<Authorization = Unauthorized> {
	auth: Authorization,
}

impl OFClient {
	pub fn new() -> Self {
		Self { auth: Unauthorized }
	}
}

impl OFClient<Unauthorized> {
	pub async fn authorize(self, params: AuthParams) -> anyhow::Result<OFClient<Authorized>> {
		let static_params = get_static_params().await?;

		let cookie_jar = Jar::default();
		let cookie = &params.cookie;
		let url: Url = "https://onlyfans.com".parse()?;

		cookie_jar.add_cookie_str(&format!("auth_id={}", &cookie.auth_id), &url);
		cookie_jar.add_cookie_str(&format!("sess={}", &cookie.sess), &url);
		cookie_jar.add_cookie_str(&format!("auth_hash={}", &cookie.auth_hash), &url);

		let mut headers = header::HeaderMap::new();
		headers.insert(header::USER_AGENT, header::HeaderValue::from_str(&params.user_agent)?);
		headers.insert("x-bc", header::HeaderValue::from_str(&params.x_bc)?);
		headers.insert("user-id", header::HeaderValue::from_str(&params.cookie.auth_id)?);

		for remove_header in static_params.remove_headers {
			headers.remove(remove_header);
		}

		headers.insert("app-token", header::HeaderValue::from_str(&static_params.app_token)?);

		let client = reqwest::Client::builder()
		.cookie_store(true)
		.cookie_provider(Arc::new(cookie_jar))
		.gzip(true)
		.default_headers(headers)
		.build()?;

		Ok(OFClient {
			auth: Authorized { client, params }
		})
	}
}

impl OFClient<Authorized> {
	pub async fn make_headers(&self, link: &str) -> anyhow::Result<header::HeaderMap> {
		let static_params = get_static_params().await?;

		let mut headers = header::HeaderMap::new();

		headers.insert("accept", header::HeaderValue::from_static("application/json, text/plain, */*"));
		headers.insert("connection", header::HeaderValue::from_static("keep-alive"));

		let parsed_url = Url::parse(link)?;
		let mut path = parsed_url.path().to_owned();
		let query = parsed_url.query();

		let mut auth_id = "0";
		if let Some(q) = query {
			auth_id = &self.auth.params.cookie.auth_id;
			path = format!("{path}?{q}");
			headers.insert("user-id", header::HeaderValue::from_str(auth_id)?);
		}

		let time = SystemTime::now()
			.duration_since(UNIX_EPOCH)?
			.as_secs()
			.to_string();

		let msg = [&static_params.static_param, &time, &path, auth_id].join("\n");
		let mut hasher = Sha1::new();
		hasher.input_str(&msg);

		let sha1_sign = hasher.result_str();
		let sha1_b = sha1_sign.as_bytes();

		let checksum = static_params
			.checksum_indexes
			.iter()
			.map(|x| sha1_b[*x as usize] as i32)
			.sum::<i32>() + static_params.checksum_constant;

		headers.insert("sign", header::HeaderValue::from_str(
				&static_params.format
					.replacen("{}", &sha1_sign, 1)
					.replacen("{:x}", &format!("{:x}", checksum.abs()), 1)
			)?,
		);
		headers.insert("time", header::HeaderValue::from_str(&time)?);

		Ok(headers)
	}

	pub async fn get(&self, link: &str) -> anyhow::Result<Response> {
		let iget = || async move {
			let headers = self.make_headers(link).await?;

			self.auth.client.get(link)
			.headers(headers)
			.send()
			.await
			.and_then(Response::error_for_status)
			.inspect_err(|err| error!("Error fetching {link}: {err:?}"))
			.map_err(Into::into)
		};

		Retry::spawn(FixedInterval::from_millis(1000).take(5), iget).await
	}

	pub async fn post(&self, link: &str) -> anyhow::Result<Response> {
		let ipost = || async move {
			let headers = self.make_headers(link).await?;
	
			self.auth.client.post(link)
			.headers(headers)
			.send()
			.await
			.and_then(Response::error_for_status)
			.inspect_err(|err| error!("Error posting to {link}: {err:?}"))
			.map_err(Into::into)
		};

		Retry::spawn(FixedInterval::from_millis(1000).take(5), ipost).await
	}

	pub async fn get_user(&self, user_id: &(impl fmt::Display + std::marker::Sync)) -> anyhow::Result<User> {
		self.get(&format!("https://onlyfans.com/api2/v2/users/{user_id}"))
		.and_then(|response| response.json::<User>().map_err(Into::into))
		.await
		.inspect(|user| info!("Got user: {:?}", user))
		.inspect_err(|err| error!("Error reading user {user_id}: {err:?}"))
	}

	pub async fn get_post(&self, post_id: &(impl fmt::Display + std::marker::Sync)) -> anyhow::Result<PostContent> {
		self.get(&format!("https://onlyfans.com/api2/v2/posts/{post_id}"))
		.and_then(|response| response.json::<PostContent>().map_err(Into::into))
		.await
		.inspect(|content| info!("Got content: {:?}", content))
		.inspect_err(|err| error!("Error reading content {post_id}: {err:?}"))
	}

	pub async fn like_post(&self, post: &PostContent) -> anyhow::Result<()> {
		let user_id = post.author.id;
		let post_id = post.id;

		self.post(&format!("https://onlyfans.com/api2/v2/posts/{post_id}/favorites/{user_id}"))
		.await
		.map(|_| ())
	}
	
	pub async fn like_message(&self, message: &MessageContent) -> anyhow::Result<()> {
		let message_id = message.id;

		self.post(&format!("https://onlyfans.com/api2/v2/messages/{message_id}/like"))
		.await
		.map(|_| ())
	}

	pub async fn like_story(&self, story: &StoryContent) -> anyhow::Result<()> {
		let story_id = story.id;

		self.post(&format!("https://onlyfans.com/api2/v2/stories/{story_id}/like"))
		.await
		.map(|_| ())
	}


	pub async fn fetch_file(&self, url: &str, path: &Path, filename: Option<&str>) -> anyhow::Result<(bool, PathBuf)> {
		let parsed_url: Url = url.parse()?;
		let full = filename
		.or_else(|| {
			parsed_url
			.path_segments()
			.and_then(Iterator::last)
			.and_then(|name| (!name.is_empty()).then_some(name))
		})
		.ok_or_else(|| anyhow!("Filename unknown"))?;
	
	let (filename, _) = full.rsplit_once('.').expect("Split extension from filename");
	
	let full_path = path.join(full);
	
		if !full_path.exists() {
			fs::create_dir_all(path)?;
			let temp_path = path.join(filename).with_extension(".path");
			let mut f = File::create(&temp_path).context(format!("Created file at {:?}", temp_path))?;

			self.get(url)
			.and_then(|response| async move {
				let mut stream = response.bytes_stream();
				while let Some(item) = stream.next().await {
					let chunk = item.context("Error while downloading file")?;
					f.write_all(&chunk).context("Error writing file")?;
				}
				Ok(())
			})
			.await
			.inspect_err(|err| error!("{err:?}"))
			.and_then(|_| fs::rename(&temp_path, &full_path).context(format!("Renamed {:?} to {:?}", temp_path, full_path)))
			.inspect_err(|err| error!("Error renaming file: {err:?}"))?;
		}

		Ok((full_path.exists(), full_path))
	}
}