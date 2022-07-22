#![allow(dead_code)]

use std::fs;
use std::fmt;
use std::sync::Arc;
use crypto::sha1::Sha1;
use serde::{Deserialize};
use crypto::digest::Digest;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use reqwest::{cookie::Jar, Url, header, Response, Error};

pub struct Cookie {
	pub auth_id: String,
	sess: String,
	auth_hash: String,
	auth_uniq_: String,
	auth_uid_: String,
}

impl fmt::Display for Cookie {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		f.write_str(&format!("auth_id={}; ", self.auth_id))?;
		f.write_str(&format!("sess={}; ", self.sess))?;
		f.write_str(&format!("auth_hash={}; ", self.auth_hash))?;
		f.write_str(&format!("auth_uniq_={}; ", self.auth_uniq_))?;
		f.write_str(&format!("auth_uid_={};", self.auth_uid_))?;

		Ok(())
	}
}

#[derive(Deserialize)]
struct StaticParams {
	static_param: String,
	format: String,
	checksum_indexes: Vec<i32>,
	checksum_constants: Vec<i32>,
	checksum_constant: i32,
	app_token: String,
	remove_headers: Vec<String>,
	error_code: i32,
	message: String
}

#[derive(Deserialize)]
struct _Authparams {
	cookie: String,
	x_bc: String,
	user_agent: String
}

#[derive(Deserialize)]
struct AuthParams {
	auth: _Authparams
}

async fn get_params() -> (StaticParams, AuthParams) {
	(
		serde_json::from_str(
			&reqwest::get("https://raw.githubusercontent.com/DATAHOARDERS/dynamic-rules/main/onlyfans.json")
			.await.unwrap()
			.text()
			.await.unwrap()
		).unwrap(),
		serde_json::from_str(
			&fs::read_to_string("auth.json").unwrap()
		).unwrap()
	)
}

fn parse_cookie(cookie: &str) -> Cookie {
	let mut cookie_map: HashMap<&str, &str> = HashMap::new();
	let filtered_str = cookie.replace(';', "");
	for c in filtered_str.split(' ') {
		let split_cookie: Vec<&str> = c.split('=').collect();
		cookie_map.insert(split_cookie[0], split_cookie[1]);
	}

	Cookie {
		auth_id: cookie_map.get("auth_id").unwrap_or(&"").to_string(),
		sess: cookie_map.get("sess").unwrap_or(&"").to_string(),
		auth_hash: cookie_map.get("auth_hash").unwrap_or(&"").to_string(),
		auth_uniq_: cookie_map.get("auth_uniq_").unwrap_or(&"").to_string(),
		auth_uid_: cookie_map.get("auth_uid_").unwrap_or(&"").to_string()
	}
}

fn make_headers(link: &str, s_params: &StaticParams, a_params: &AuthParams) -> header::HeaderMap {
	let mut headers: header::HeaderMap = header::HeaderMap::new();
	let cookies: Cookie = parse_cookie(&a_params.auth.cookie);

	headers.insert(header::USER_AGENT, header::HeaderValue::from_str(&a_params.auth.user_agent).unwrap());
	headers.insert(header::REFERER, header::HeaderValue::from_str(&link).unwrap());
	headers.insert("x-bc", header::HeaderValue::from_str(&a_params.auth.x_bc).unwrap());
	headers.insert("user-id", header::HeaderValue::from_str(&cookies.auth_id).unwrap());
	
	for remove_header in s_params.remove_headers.iter() {
		headers.remove(remove_header);
	}
	
	if link.contains("https://onlyfans.com/api2/v2/") {
		headers.insert("app-token", header::HeaderValue::from_str(&s_params.app_token).unwrap());
		
		let parsed_url = Url::parse(link).unwrap();
		let mut path = parsed_url.path().to_owned();
		let query = parsed_url.query();

        let mut auth_id = "0";
        match query {
            Some(q) => {
                auth_id = &cookies.auth_id;
                path = format!("{}?{}", path, q);
                headers.insert("user-id", header::HeaderValue::from_str(auth_id).unwrap());
            },
            None => ()
        }
		
		let time: String = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs().to_string();
		let msg: String = [&s_params.static_param, &time, &path, auth_id].join("\n");
		let mut hasher = Sha1::new();
		hasher.input_str(&msg);
		
		let sha1_sign = hasher.result_str();
		let sha1_b = sha1_sign.as_bytes();
		
		let checksum: i32 = s_params.checksum_indexes.iter().map(|x: &i32| -> i32 { sha1_b[*x as usize] as i32 }).sum::<i32>() + s_params.checksum_constant;
		
		let sign_parameters: Vec<&str> = s_params.format.split(':').collect();
		headers.insert("sign", header::HeaderValue::from_str(&&format!("{}:{}:{:x}:{}",sign_parameters[0], sha1_sign, checksum.abs(), sign_parameters[4])).unwrap());
		headers.insert("time", header::HeaderValue::from_str(&time).unwrap());
	}
	headers
}

pub async fn get_json(link: &str) -> Result<Response, Error> {
    let (static_params, auth_params) = get_params().await;

	let cookie_jar = Jar::default();
	let cookie = parse_cookie(&auth_params.auth.cookie);
	let empty: Url = link.parse().unwrap();

	cookie_jar.add_cookie_str(&format!("auth_hash={}", &cookie.auth_hash), &empty);
	cookie_jar.add_cookie_str(&format!("auth_id={}", &cookie.auth_id), &empty);
	cookie_jar.add_cookie_str(&format!("auth_uid_={}", &cookie.auth_uid_), &empty);
	cookie_jar.add_cookie_str(&format!("auth_uniq_={}", &cookie.auth_uniq_), &empty);
	cookie_jar.add_cookie_str(&format!("sess={}", &cookie.sess), &empty);

	let client = reqwest::Client::builder()
		.cookie_store(true)
		.cookie_provider(Arc::new(cookie_jar))
		.gzip(true)
		.default_headers(make_headers(link, &static_params, &auth_params))
		.build().unwrap();

    client.get(link)
		.header("accept", "application/json, text/plain, */*")
		.header("connection", "keep-alive")
		.send().await
}