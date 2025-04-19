use std::{error::Error, time::Duration};

use clap::Parser;
use discord_rich_presence::{
    DiscordIpc, DiscordIpcClient,
    activity::{Activity, ActivityType, Assets, Button, Timestamps},
};
use log::{error, info, warn};
use size_parser::SizeParser;
use tokio::sync::mpsc;
use track_info::TrackInfo;
use urlencoding::encode;

const APPLICATION_ID: &str = "1210361074247802940";
const DESCRIPTION: &str = "\n\nDiscord presence for ravachol/kew, or any MPRIS compatible music player.\n
Note that activity buttons might not be visible to the user who sets the activity, but they are to everyone else.
This is a Discord issue, see https://github.com/Mastermindzh/tidal-hifi/issues/429#issuecomment-2504798129.";

#[derive(Parser)]
#[command(version, author, about = DESCRIPTION)]
struct App {
    #[arg(short, long)]
    verbose: bool,

    #[arg(
        short,
        long,
        default_value_t = 3,
        help = "how often to retry if we get an ipc error"
    )]
    retries: usize,

    #[arg(
        short,
        long,
        default_value = "kew",
        help = "name of the music player to follow (see `playerctl`)"
    )]
    player: String,

    #[arg(
        short = 'i',
        long,
        default_value = APPLICATION_ID,
        help = "Discord application ID",
        long_help = "Discord application ID\nsee https://discord.com/developers/applications"
    )]
    app_id: String,

    #[arg(long, help = "hide the button of the music_presence github repo")]
    hide_repository_button: bool,

    #[arg(long, help = "do not resize local track covers before uploading them")]
    skip_resizing: bool,

    #[arg(
        long,
        value_parser = SizeParser,
        help = "{width}x{height} to which track covers get resized before uploading",
        default_value = "150x150"
    )]
    size: (u32, u32),

    #[arg(skip)]
    track: TrackInfo,

    #[arg(skip)]
    client: Option<DiscordIpcClient>,
}

#[tokio::main]
async fn main() {
    let mut args = App::parse();

    if args.verbose {
        env_logger::builder()
            .filter_level(log::LevelFilter::Trace)
            .init();
    } else {
        env_logger::init();
    }

    let (sx, mut rx) = mpsc::unbounded_channel();

    let player = args.player.clone();
    let resize = (!args.skip_resizing).then_some(args.size);
    tokio::spawn(async move {
        if let Err(e) = media_listener::subscribe(sx, player, resize).await {
            error!("Failed to listen to playerctl due to critical error: {e}");
        }
    });

    while let Some(update) = rx.recv().await {
        for i in 0..args.retries {
            if let Err(e) = args.handle(update.clone()) {
                error!("Received an error while handling TrackUpdate: {e}");
                args.client = None;
                if i < args.retries - 1 {
                    info!("Retrying in 1 second.");
                    tokio::time::sleep(Duration::from_secs(1)).await;
                } else {
                    warn!("Max. retries reached! {update:?} will be ignored");
                }
            } else {
                break;
            }
        }
    }

    warn!("Sender dropped, exiting");
}

impl App {
    fn handle(&mut self, update: TrackUpdate) -> Result<(), Box<dyn Error>> {
        match update {
            TrackUpdate::New(new_track) => {
                if new_track.paused {
                    info!("Track is paused, removing activity status");
                    self.clear_activity()?;
                } else if new_track != self.track {
                    info!("Playing {} by {}", new_track.title, new_track.artist);
                    self.track = new_track;
                    self.set_activity()?;
                } else {
                    info!("Track got unpaused, restarting activity");
                    self.set_activity()?;
                }
            }
            TrackUpdate::ImageUploaded(url) => {
                info!("Done uploading the cover image");
                self.track.art_url = url;
                self.set_activity()?;
            }
            TrackUpdate::None => {
                info!("No more tracks are playing");
                self.clear_activity()?;
            }
        }
        Ok(())
    }

    fn set_activity(&mut self) -> Result<(), Box<dyn Error>> {
        let c = match self.client.as_mut() {
            Some(c) => c,
            None => {
                let mut c = DiscordIpcClient::new(&self.app_id)?;
                c.connect()?;
                self.client = Some(c);
                self.client.as_mut().unwrap()
            }
        };

        let state_fmt = format!(
            "by: {}{}",
            self.track.artist,
            (!self.track.album.is_empty())
                .then(|| format!(", in: {}", self.track.album))
                .unwrap_or_default()
        );

        let timestamps = Timestamps::new()
            .start(self.track.start)
            .end(self.track.start + self.track.length / 1000);

        let fmt = format!("{} {}", self.track.title, self.track.artist);
        let query = encode(&fmt);
        let url = format!("https://yewtu.be/search?q={query}&type=video");

        let mut buttons = vec![Button::new("Listen along", &url)];
        if !self.hide_repository_button {
            buttons.push(Button::new(
                "View repository",
                "https://github.com/faervan/music_presence",
            ));
        }

        let activity = Activity::new()
            .state(&state_fmt)
            .details(&self.track.title)
            .assets(Assets::new().large_image(&self.track.art_url))
            .activity_type(ActivityType::Listening)
            .timestamps(timestamps)
            .buttons(buttons);

        c.set_activity(activity)?;

        Ok(())
    }

    fn clear_activity(&mut self) -> Result<(), Box<dyn Error>> {
        if let Some(c) = self.client.as_mut() {
            c.clear_activity()?;
            c.close()?;
            self.client = None;
        }

        Ok(())
    }
}

#[derive(Clone, Debug)]
enum TrackUpdate {
    New(TrackInfo),
    ImageUploaded(String),
    /// No more tracks are playing
    None,
}

mod size_parser {
    use std::ffi::OsStr;

    use clap::builder::TypedValueParser;

    #[derive(Clone)]
    pub struct SizeParser;

    impl TypedValueParser for SizeParser {
        type Value = (u32, u32);

        fn parse_ref(
            &self,
            _cmd: &clap::Command,
            _arg: Option<&clap::Arg>,
            value: &OsStr,
        ) -> Result<Self::Value, clap::Error> {
            let value = value
                .to_str()
                .ok_or(clap::Error::new(clap::error::ErrorKind::InvalidUtf8))?;
            if let Some((x, y)) = value.split_once('x') {
                if let Ok(x) = x.parse() {
                    if let Ok(y) = y.parse() {
                        return Ok((x, y));
                    }
                }
            }
            Err(clap::Error::new(clap::error::ErrorKind::ValueValidation))
        }
    }
}

mod media_listener {
    use std::{error::Error, process::Stdio};

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

    pub async fn subscribe(
        sender: UnboundedSender<TrackUpdate>,
        player: String,
        resize: Option<(u32, u32)>,
    ) -> Result<(), Box<dyn Error>> {
        let format = "'{ \
           \"title\": \"{{title}}\", \
           \"artist\": \"{{artist}}\", \
           \"album\": \"{{album}}\", \
           \"art_url\": \"{{mpris:artUrl}}\", \
           \"length\": \"{{mpris:length}}\", \
           \"status\": \"{{status}}\", \
           \"player\": \"{{playerName}}\" \
        }'";
        let Ok(mut child) = Command::new("sh")
            .arg("-c")
            .arg(format!(
                "playerctl \
                    --follow metadata \
                    --player {player} \
                    --format {format}",
            ))
            .stdout(Stdio::piped())
            .spawn()
        else {
            return Err("Failed to spawn playerctl. Are you sure it is installed?".into());
        };

        let stdout = child
            .stdout
            .take()
            .ok_or("Child command has no handle to stdout")?;

        let mut reader = BufReader::new(stdout).lines();
        let mut last_track = String::new();

        loop {
            let Some(line) = reader.next_line().await.ok().flatten() else {
                return Err("The playerctl child command reached EOF unexpectedly".into());
            };
            if let Ok(track) = serde_json::from_str::<TrackInfo>(&line) {
                // If cover art is local, we need to upload first
                if let Some(url) = track.art_is_local.then_some(track.art_url.clone()) {
                    if url != last_track {
                        last_track = url.clone();
                        let sender = sender.clone();
                        tokio::task::spawn(async move {
                            if let Err(e) = upload_cover(sender, &url, resize).await {
                                error!("Failed to upload image cover: {e:?}");
                            }
                        });
                    }
                }
                sender.send(TrackUpdate::New(track))?;
            } else if matches!(line.trim(), "") {
                sender.send(TrackUpdate::None)?;
            }
        }
    }

    async fn upload_cover(
        sender: UnboundedSender<TrackUpdate>,
        mut url: &str,
        resize: Option<(u32, u32)>,
    ) -> Result<(), Box<dyn Error>> {
        if let Some(size) = resize {
            let image = image::ImageReader::open(url)?.decode()?;
            if size.0 > image.width() && size.1 > image.height() {
                url = "/tmp/music_presence_tmp_cover.jpg";
                image
                    .resize_to_fill(size.0, size.1, image::imageops::FilterType::Triangle)
                    .save(url)?;
            }
        }
        let Ok(form) = reqwest::multipart::Form::new().file("file", url).await else {
            if !std::fs::exists(url).is_ok_and(|b| b) {
                warn!("File {url} does not exist or is a broken symlink.");
            }
            return Err("Failed to create reqwest::multipart::Form".into());
        };
        let response = reqwest::Client::new()
            .post("https://tmpfiles.org/api/v1/upload")
            .multipart(form)
            .send()
            .await?;
        let img_url = response.json::<ResponseBody>().await?.data.url.replacen(
            "https://tmpfiles.org/",
            "https://tmpfiles.org/dl/",
            1,
        );
        info!("got url: {img_url}");
        sender.send(TrackUpdate::ImageUploaded(img_url))?;

        Ok(())
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
        pub paused: bool,
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
                paused: map
                    .get("status")
                    .map(|v| !matches!(v.as_str(), Some("Playing")))
                    .unwrap_or(true),
            })
        }
    }

    impl PartialEq for TrackInfo {
        fn eq(&self, other: &Self) -> bool {
            self.title == other.title
                && self.artist == other.artist
                && self.album == other.album
                && self.art_is_local == other.art_is_local
                && self.length == other.length
        }
    }
}
