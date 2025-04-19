# Music presence
Show them what music you listen to, even when not using Spotify.<br>
`music_presence` was made for [kew](https://github.com/ravachol/kew), but works with any players supporting [MPRIS](https://specifications.freedesktop.org/mpris-spec/latest/).

It works by subscribing to `playerctl` for MPRIS events and uploading the cover art of the playing media to [tmpfiles.org](https://tmpfiles.org/) if it is stored locally, because Discords RPC requires image assets to be provided as web urls.

![image](https://github.com/user-attachments/assets/919ddf71-7254-4cf2-b78f-07d2166a0c91)

## Building from source
```sh
# Clone this repository
git clone https://github.com/Faervan/music_presence.git
cd music_presence

# Build using cargo (install rustup.rs if you don't have it installed already)
cargo build --release

# Install the binary
sudo cp target/release/music_presence /usr/local/bin

# Optional: you may remove this directory after you copied the binary to /usr/local/bin
cd ..
rm -r music_presence
```

## Uninstall
Applies if you followed the steps from [Building from source](#building-from-source):
```sh
sudo rm /usr/local/bin/music_presence
```

## Usage
After installing, just add `music_presence` to your autostart.

CLI options:
```
$ ./target/release/music_presence -h

Discord presence for ravachol/kew, or any MPRIS compatible music player.

Note that activity buttons might not be visible to the user who sets the activity, but they are to everyone else.
This is a Discord issue, see https://github.com/Mastermindzh/tidal-hifi/issues/429#issuecomment-2504798129.

Usage: music_presence [OPTIONS]

Options:
  -v, --verbose                 
  -r, --retries <RETRIES>       how often to retry if we get an ipc error [default: 3]
  -p, --player <PLAYER>         name of the music player to follow (see `playerctl`) [default: kew]
  -i, --app-id <APP_ID>         Discord application ID [default: 1210361074247802940]
      --hide-repository-button  hide the button of the music_presence github repo
      --skip-resizing           do not resize local track covers before uploading them
      --size <SIZE>             {width}x{height} to which track covers get resized before uploading [default: 150x150]
  -h, --help                    Print help (see more with '--help')
  -V, --version                 Print version
```

Note that when changing the player from `kew` to smth else (e.g. `spotify`), `music_presence` will still show up as "Listening to kew.m3u" because the Discord application with ID `1210361074247802940` has the name "kew.m3u".
Head over to [Discords developer portal](https://discord.com/developers/applications) to create your own Discord application and pass its ID to `--app-id`.

## Credits
`music_presence` is powered by all the awesome crates listed in [Cargo.toml](Cargo.toml).
Not listed there are `playerctl` and [tmpfiles.org](https://tmpfiles.org/), on which `music_presence` is built upon as well.
