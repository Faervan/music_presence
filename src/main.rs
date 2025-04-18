use std::error::Error;

use discord_rich_presence::{
    DiscordIpc, DiscordIpcClient,
    activity::{Activity, ActivityType, Assets, Button, Timestamps},
};
use log::{error, info, warn};
use tokio::sync::mpsc;
use track_info::TrackInfo;
use urlencoding::encode;

const CLIENT_ID: &str = "1210361074247802940";
const MAX_RETRIES: usize = 3;

#[tokio::main]
async fn main() {
    env_logger::init();

    let (sx, mut rx) = mpsc::unbounded_channel();

    tokio::spawn(media_listener::subscribe(sx));

    let mut track = TrackInfo::default();
    let mut client = None;

    while let Some(update) = rx.recv().await {
        for _ in 0..MAX_RETRIES {
            if let Err(e) = handle(update.clone(), &mut track, &mut client) {
                error!("Received an error while handling TrackUpdate: {e}");
                client = None;
            } else {
                break;
            }
        }
    }

    warn!("Sender dropped, exiting");
}

fn handle(
    update: TrackUpdate,
    track: &mut TrackInfo,
    client: &mut Option<DiscordIpcClient>,
) -> Result<(), Box<dyn Error>> {
    match update {
        TrackUpdate::New(new_track) => {
            info!("Playing {} by {}", new_track.title, new_track.artist);
            *track = new_track;
            set_activity(client, track)?;
        }
        TrackUpdate::ImageUploaded(url) => {
            info!("Done uploading the cover image");
            track.art_url = url;
            set_activity(client, track)?;
        }
        TrackUpdate::None => {
            info!("No more tracks are playing");
            if let Some(c) = client.as_mut() {
                c.clear_activity()?;
                c.close()?;
                *client = None;
            }
        }
    }
    Ok(())
}

fn set_activity(
    client: &mut Option<DiscordIpcClient>,
    track: &TrackInfo,
) -> Result<(), Box<dyn Error>> {
    let c = match client.as_mut() {
        Some(c) => c,
        None => {
            let mut c = DiscordIpcClient::new(CLIENT_ID)?;
            c.connect()?;
            *client = Some(c);
            client.as_mut().unwrap()
        }
    };

    let state_fmt = format!(
        "by: {}{}",
        track.artist,
        (!track.album.is_empty())
            .then(|| format!(", in: {}", track.album))
            .unwrap_or_default()
    );

    let timestamps = Timestamps::new()
        .start(track.start)
        .end(track.start + track.length / 1000);

    let fmt = format!("{} {}", track.title, track.artist);
    let query = encode(&fmt);
    let url = format!("https://yewtu.be/search?q={query}&type=video");

    let activity = Activity::new()
        .state(&state_fmt)
        .details(&track.title)
        .assets(Assets::new().large_image(&track.art_url))
        .activity_type(ActivityType::Listening)
        .timestamps(timestamps)
        .buttons(vec![Button::new("Listen along", &url)]);

    c.set_activity(activity)?;

    Ok(())
}

#[derive(Clone)]
enum TrackUpdate {
    New(TrackInfo),
    ImageUploaded(String),
    /// No more tracks are playing
    None,
}

mod media_listener {
    use std::process::Stdio;

    use log::{error, info, warn};
    use serde::Deserialize;
    use tokio::{
        io::{AsyncBufReadExt, BufReader},
        process::Command,
        sync::mpsc::UnboundedSender,
    };

    use crate::{TrackUpdate, track_info::TrackInfo};

    #[derive(Deserialize)]
    #[allow(dead_code)]
    struct ResponseBody {
        status: String,
        data: ResponseData,
    }

    #[derive(Deserialize)]
    struct ResponseData {
        url: String,
    }

    pub async fn subscribe(sender: UnboundedSender<TrackUpdate>) {
        let mut child = Command::new("sh")
            .arg("-c")
            .arg(
                "playerctl \
                    --follow metadata \
                    -p kew \
                    --format '{ \
                       \"title\": \"{{title}}\", \
                       \"artist\": \"{{artist}}\", \
                       \"album\": \"{{album}}\", \
                       \"art_url\": \"{{mpris:artUrl}}\", \
                       \"length\": \"{{mpris:length}}\", \
                       \"status\": \"{{status}}\", \
                       \"player\": \"{{playerName}}\" \
                    }'",
            )
            .stdout(Stdio::piped())
            .spawn()
            .expect("Failed to read output from playerctl");

        let stdout = child
            .stdout
            .take()
            .expect("child did not have a handle to stdout");

        let mut reader = BufReader::new(stdout).lines();
        let mut last_track = String::new();

        loop {
            let Some(line) = reader.next_line().await.ok().flatten() else {
                return;
            };
            if let Ok(track) = serde_json::from_str::<TrackInfo>(&line) {
                // If cover art is local, we need to upload first
                if let Some(url) = track.art_is_local.then_some(track.art_url.clone()) {
                    if url != last_track {
                        last_track = url.clone();
                        let sender = sender.clone();
                        tokio::task::spawn(async move {
                            let Ok(form) = reqwest::multipart::Form::new().file("file", &url).await
                            else {
                                if !std::fs::exists(&url).is_ok_and(|b| b) {
                                    warn!("File {url} does not exist or is a broken symlink.");
                                }
                                error!("Failed to create reqwest::multipart::Form");
                                return;
                            };
                            let response = reqwest::Client::new()
                                .post("https://tmpfiles.org/api/v1/upload")
                                .multipart(form)
                                .send()
                                .await
                                .unwrap();
                            let img_url = response
                                .json::<ResponseBody>()
                                .await
                                .unwrap()
                                .data
                                .url
                                .replacen("https://tmpfiles.org/", "https://tmpfiles.org/dl/", 1);
                            info!("got url: {img_url}");
                            sender.send(TrackUpdate::ImageUploaded(img_url)).unwrap();
                        });
                    }
                }
                sender.send(TrackUpdate::New(track)).unwrap();
            } else if matches!(line.trim(), "") {
                sender.send(TrackUpdate::None).unwrap();
            }
        }
    }
}

mod track_info {
    use std::time::{SystemTime, UNIX_EPOCH};

    use serde::Deserialize;

    #[derive(Debug, Default, Clone)]
    pub(crate) struct TrackInfo {
        pub title: String,
        pub artist: String,
        pub album: String,
        pub art_url: String,
        pub _player: String,
        pub art_is_local: bool,
        pub start: i64,
        pub length: i64,
        pub _paused: bool,
    }

    impl<'de> Deserialize<'de> for TrackInfo {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            let map: serde_json::Map<String, serde_json::Value> =
                Deserialize::deserialize(deserializer)?;

            let mut art_url = map
                .get("art_url")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();

            let art_is_local = match art_url.strip_prefix("file://") {
                Some(file) => {
                    art_url = file.to_string();
                    true
                }
                None => false,
            };

            Ok(TrackInfo {
                title: map
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                artist: map
                    .get("artist")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                album: map
                    .get("album")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                _player: map
                    .get("player")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                art_url,
                art_is_local,
                start: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as i64,
                length: map
                    .get("length")
                    .and_then(|v| v.as_str().and_then(|s| s.parse::<i64>().ok()))
                    .unwrap_or_default(),
                _paused: map
                    .get("status")
                    .map(|v| !matches!(v.as_str(), Some("Playing")))
                    .unwrap_or(true),
            })
        }
    }
}
