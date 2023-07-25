#![allow(dead_code)]

pub mod socket;

use of_client::content;
use winrt_toast::{Toast, content::text::TextPlacement, Text};

pub enum ContentType {
    Posts,
    Messages,
    Stories,
    Notifications,
    Streams
}

impl ToString for ContentType {
    fn to_string(&self) -> String {
        match self {
            ContentType::Posts => "Posts",
            ContentType::Messages => "Messages",
            ContentType::Stories => "Stories",
            ContentType::Notifications => "Notifications",
            ContentType::Streams => "Streams",
        }.to_string()
    }
}

pub trait ToToast {
    fn to_toast(&self) -> Toast;
    fn header() -> ContentType;
}

impl ToToast for content::Post {
    fn to_toast(&self) -> Toast {
        let mut toast = Toast::new();
		toast.text2(&self.raw_text);

		if let Some(price) = self.price && price > 0f32 {
			toast
			.text3(Text::new(format!("${price:.2}"))
			.with_placement(TextPlacement::Attribution));
		}

		toast
    }

    fn header() -> ContentType {
        ContentType::Posts
    }
}

impl ToToast for content::Chat {
    fn to_toast(&self) -> Toast {
        let mut toast = Toast::new();
		toast.text2(&self.text);

		if let Some(price) = self.price && price > 0f32 {
			toast
			.text3(Text::new(format!("${price:.2}"))
			.with_placement(TextPlacement::Attribution));
		}

		toast
    }

    fn header() -> ContentType {
        ContentType::Messages
    }
}

impl ToToast for content::Story {
    fn to_toast(&self) -> Toast {
        Toast::new()
    }

    fn header() -> ContentType {
        ContentType::Stories
    }
}

impl ToToast for content::Notification {
    fn to_toast(&self) -> Toast {
        let mut toast = Toast::new();
		toast.text2(&self.text);
		
		toast
    }

    fn header() -> ContentType {
        ContentType::Notifications
    }
}

impl ToToast for content::Stream {
    fn to_toast(&self) -> Toast {
        let mut toast = Toast::new();

		toast
		.text2(&self.title)
		.text3(&self.description);

		toast
    }

    fn header() -> ContentType {
        ContentType::Streams
    }
}