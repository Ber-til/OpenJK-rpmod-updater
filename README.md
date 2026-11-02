# OpenJK-rpmod Updater

Installs [OpenJK-rpmod](https://github.com/Ber-til/OpenJK-rpmod) into Jedi Academy, and keeps it up to date. Windows, Linux and macOS.

Open it and it looks for Jedi Academy on your computer, and asks GitHub for the newest OpenJK-rpmod:

- **Not installed yet:** it asks whether to install it. If Jedi Academy is in more than one place, you pick which one first.
- **Installed:** it tells you when you're up to date. When a newer release is out, it shows what changed since your version and asks whether to update.

Nothing else is needed: no installer, no settings.

## Where it looks

- **Windows:** Jedi Academy with RPMod-Launcher in `GameData\rpmod`, from Steam, GOG, the CD or a folder like `C:\JKA`. It looks two folders deep on every drive.
- **Linux:** Steam (including Flatpak and Snap), `~/JKA`, three folders deep in your home folder, and `/opt`.
- **macOS:** Steam, `~/JKA`, your home folder and `/Applications`.

A Jedi Academy is a `GameData` folder with `base/assets0.pk3` in it. If yours isn't found, **Use another folder...** lets you pick it.

## What it does to your game

- It downloads the release for your system (`OpenJK-windows-x86.zip`, `OpenJK-linux-x86.tar.gz` or `OpenJK-macos-<arm64|x86_64>.tar.gz`) and unpacks it into `GameData`.
- The first time, the game's files it replaces are kept in `GameData/OpenJK-rpmod-backup`.
- It won't change anything while Jedi Academy is running.
- On Windows, if the game is in a folder that needs administrator rights (like `Program Files`), it offers to restart as administrator.

It reads the version you have from the game itself (`openjk.x86.exe` carries `OpenJK-rpmod: v0.7.3`), so it needs no files of its own.

## Running it

- **Windows:** `OpenJK-rpmod-Updater.exe`. Windows may warn about an unknown program: **More info**, then **Run anyway**.
- **Linux:** extract `OpenJK-rpmod-Updater-linux.tar.gz` and run `OpenJK-rpmod-Updater`.
- **macOS:** extract `OpenJK-rpmod-Updater-macos.zip`. The app isn't signed, so the first time right-click it and pick **Open**.

## Building

It's Rust, with [egui](https://github.com/emilk/egui) for the window.

```
cargo build --release
cargo test                 # the version comparisons
cargo test -- --ignored    # installs the newest release into a made-up GameData (downloads it)
```

On Linux the window needs `libxkbcommon-dev libgl1-mesa-dev libwayland-dev libx11-dev libxcursor-dev libxrandr-dev libxi-dev`.

GitHub Actions builds all three on every push, and makes a release when a `v*` tag is pushed.
