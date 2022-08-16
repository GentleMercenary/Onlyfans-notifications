use super::message_types::{self, Error};

use async_trait::async_trait;
use cached::proc_macro::once;
use crypto::{digest::Digest, sha1::Sha1};
use futures::TryFutureExt;
use reqwest::{cookie::Jar, header, Client, Response, Url};
use serde::{Deserialize, Deserializer};
use std::{
	collections::HashMap,
	fmt, fs,
	fs::File,
	io::Cursor,
	path::{Path, PathBuf},
	sync::Arc,
	time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Clone)]
pub struct Cookie {
	pub auth_id: String,
	sess: String,
	auth_hash: String,
}

impl fmt::Display for Cookie {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		f.write_str(&format!("auth_id={}; ", self.auth_id))?;
		f.write_str(&format!("sess={}; ", self.sess))?;
		f.write_str(&format!("auth_hash={}; ", self.auth_hash))?;

		Ok(())
	}
}

fn parse_cookie<'de, D>(deserializer: D) -> Result<Cookie, D::Error>
where
	D: Deserializer<'de>,
{
	let s = String::deserialize(deserializer)?;
	let mut cookie_map: HashMap<&str, &str> = HashMap::new();
	let filtered_str = s.replace(';', "");
	for c in filtered_str.split(' ') {
		let split_cookie: Vec<&str> = c.split('=').collect();
		cookie_map.insert(split_cookie[0], split_cookie[1]);
	}

	Ok(Cookie {
		auth_id: cookie_map.get("auth_id").unwrap_or(&"").to_string(),
		sess: cookie_map.get("sess").unwrap_or(&"").to_string(),
		auth_hash: cookie_map.get("auth_hash").unwrap_or(&"").to_string(),
	})
}

#[derive(Deserialize, Clone)]
struct StaticParams {
	static_param: String,
	format: String,
	checksum_indexes: Vec<usize>,
	// checksum_constants: Vec<i32>,
	checksum_constant: i32,
	app_token: String,
	remove_headers: Vec<String>,
	// error_code: i32,
	// message: String
}

#[derive(Deserialize, Clone)]
struct _AuthParams {
	auth: AuthParams,
}

#[derive(Deserialize, Clone)]
struct AuthParams {
	#[serde(deserialize_with = "parse_cookie")]
	cookie: Cookie,
	x_bc: String,
	user_agent: String,
}

#[once(time = 10, result = true, sync_writes = true)]
async fn get_params() -> Result<(StaticParams, AuthParams), Error> {
	Ok((
		serde_json::from_str(
			&reqwest::get(
				"https://raw.githubusercontent.com/DATAHOARDERS/dynamic-rules/main/onlyfans.json",
			)
			.and_then(|response| response.text())
			.await?,
		)?,
		fs::read_to_string("auth.json").and_then(|data| {
			serde_json::from_str::<_AuthParams>(&data)
				.map(|outer| outer.auth)
				.map_err(|err| err.into())
		})?,
	))
}

#[async_trait]
pub trait ClientExt {
	async fn with_auth() -> Result<Client, Error>;
	async fn fetch(&self, link: &str) -> Result<Response, Error>;
	async fn fetch_user(&self, user_id: &str) -> Result<message_types::User, Error>;
	async fn fetch_content(&self, post_id: &str) -> Result<message_types::Content, Error>;
	async fn fetch_file(
		&self,
		url: &str,
		path: &Path,
		filename: Option<&str>,
	) -> Result<PathBuf, Error>;
}

#[async_trait]
impl ClientExt for Client {
	async fn with_auth() -> Result<Client, Error> {
		let (static_params, auth_params) = get_params().await?;

		let cookie_jar = Jar::default();
		let cookie = &auth_params.cookie;
		let url: Url = "https://onlyfans.com".parse()?;

		cookie_jar.add_cookie_str(&format!("auth_id={}", &cookie.auth_id), &url);
		cookie_jar.add_cookie_str(&format!("sess={}", &cookie.sess), &url);
		cookie_jar.add_cookie_str(&format!("auth_hash={}", &cookie.auth_hash), &url);

		let mut headers: header::HeaderMap = header::HeaderMap::new();

		headers.insert(
			header::USER_AGENT,
			header::HeaderValue::from_str(&auth_params.user_agent)?,
		);
		headers.insert("x-bc", header::HeaderValue::from_str(&auth_params.x_bc)?);
		headers.insert(
			"user-id",
			header::HeaderValue::from_str(&auth_params.cookie.auth_id)?,
		);

		for remove_header in static_params.remove_headers.iter() {
			headers.remove(remove_header);
		}

		headers.insert(
			"app-token",
			header::HeaderValue::from_str(&static_params.app_token)?,
		);

		reqwest::Client::builder()
			.cookie_store(true)
			.cookie_provider(Arc::new(cookie_jar))
			.gzip(true)
			.timeout(Duration::from_secs(30))
			.default_headers(headers)
			.build()
			.map_err(|err| err.into())
	}

	async fn fetch(&self, link: &str) -> Result<Response, Error> {
		let (static_params, auth_params) = get_params().await?;

		let mut headers: header::HeaderMap = header::HeaderMap::new();
		let parsed_url = Url::parse(link)?;
		let mut path = parsed_url.path().to_owned();
		let query = parsed_url.query();

		let mut auth_id = "0";
		match query {
			Some(q) => {
				auth_id = &auth_params.cookie.auth_id;
				path = format!("{}?{}", path, q);
				headers.insert("user-id", header::HeaderValue::from_str(auth_id)?);
			}
			None => (),
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
			.map(|x| -> i32 { sha1_b[*x] as i32 })
			.sum::<i32>()
			+ static_params.checksum_constant;

		headers.insert(
			"sign",
			header::HeaderValue::from_str(
				&static_params.format.replacen("{}", &sha1_sign, 1).replacen(
					"{:x}",
					&format!("{:x}", checksum.abs()),
					1,
				),
			)?,
		);
		headers.insert("time", header::HeaderValue::from_str(&time)?);

		info!("Fetching url {}", link);

		self.get(link)
			.header("accept", "application/json, text/plain, */*")
			.header("connection", "keep-alive")
			.headers(headers)
			.send()
			.await
			.map_err(|err| err.into())
	}

	async fn fetch_user(&self, user_id: &str) -> Result<message_types::User, Error> {
		self.fetch(&format!("https://onlyfans.com/api2/v2/users/{}", user_id))
			.and_then(|response| async move { response.text().await.map_err(|err| err.into()) })
			.and_then(|response| async move {
				serde_json::from_str(&response).map_err(|err| err.into())
			})
			.await
			.inspect(|user| info!("Got user: {:?}", user))
	}

	async fn fetch_content(&self, post_id: &str) -> Result<message_types::Content, Error> {
		self.fetch(&format!(
			"https://onlyfans.com/api2/v2/posts/{}?skip_users=all",
			post_id
		))
		.and_then(|response| async move { response.text().await.map_err(|err| err.into()) })
		.and_then(
			|response| async move { serde_json::from_str(&response).map_err(|err| err.into()) },
		)
		.await
		.inspect(|content| info!("Got content: {:?}", content))
	}

	async fn fetch_file(
		&self,
		url: &str,
		path: &Path,
		filename: Option<&str>,
	) -> Result<PathBuf, Error> {
		let parsed_url: Url = url.parse()?;
		let filename = filename
			.or_else(|| {
				parsed_url
					.path_segments()
					.and_then(|segments| segments.last())
			})
			.ok_or("Filename unknown")?;

		let full_path = path.join(filename);

		if !full_path.exists() {
			fs::create_dir_all(&path)?;
			let mut f = File::create(&full_path)?;

			self.fetch(&url)
				.and_then(
					|response| async move { response.bytes().await.map_err(|err| err.into()) },
				)
				.await
				.and_then(|bytes| {
					std::io::copy(&mut Cursor::new(bytes), &mut f).map_err(|err| err.into())
				})
				.inspect(|byte_count| info!("Wrote {} bytes to {:?}", byte_count, full_path))?;
		}

		Ok(full_path)
	}
}
