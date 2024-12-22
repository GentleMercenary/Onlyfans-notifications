mod init;

use init::init_log;
use of_notifier::init_client;

#[tokio::test]
async fn get_subscriptions() {
	init_log();

	let client = init_client().unwrap();

	for user in client.get_subscriptions().await.unwrap() {
		println!("{user:#?}");
	}
}

#[tokio::test]
async fn get_user_name() {
	init_log();

	let client = init_client().unwrap();

	let user = client.get_user("onlyfans").await.unwrap();
	println!("{user:#?}");
}

#[tokio::test]
async fn get_user_id() {
	init_log();

	let client = init_client().unwrap();

	let user = client.get_user(15585607).await.unwrap();
	println!("{user:#?}");
}