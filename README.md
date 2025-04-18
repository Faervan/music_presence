# Music presence
Show them what music you listen to, even when not using Spotify.<br>
`music_presence` was made for [kew](https://github.com/ravachol/kew), but works with any players supporting [MPRIS](https://specifications.freedesktop.org/mpris-spec/latest/).

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
