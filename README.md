# mprisence

|           | Playing                                 | Paused                                                          |
| --------- | --------------------------------------- | --------------------------------------------------------------- |
| No icon   | ![](assets/readme/playing-noicon.png)   | ![Paused on lollypop + icon](assets/readme/paused-noicon.png)   |
| Show icon | ![](assets/readme/playing-showicon.png) | ![Paused on lollypop + icon](assets/readme/paused-showicon.png) |
| No cover  | ![](assets/readme/playing-nocover.png)  | ![Paused on lollypop + icon](assets/readme/paused-nocover.png)  |

A Discord Rich Presence client for MPRIS-compatible media players with album/song cover art support

## Installation

### With [cargo](https://doc.rust-lang.org/cargo/getting-started/installation.html)

```bash
  cargo install --git https://github.com/phusitsom/mprisence.git
```

To enable **cover art support**, [see below](#cover-art-support).

## Usage

To start mprisence, simply run this command:

```bash
mprisence
```

## Configuration

The rich presence can be configured to the user's preference by providing the configuration file at `~/.config/mprisence/config.toml` or `$XDG_CONFIG_HOME/mprisence/config.toml`.

See [documentation](https://github.com/phusitsom/mprisence/wiki/Configuration/) for more advanced configuration.

- [Example config file](config/example.toml)
- [Default config file](config/default.toml)

To download example config file:

```bash
CONFIG_PATH="${XDG_CONFIG_HOME:-$HOME/.config}/mprisence/config.toml"
[ ! -f "$CONFIG_PATH" ] && curl -o "$CONFIG_PATH" --create-dirs "https://raw.githubusercontent.com/phusitsom/mprisence/main/config/example.toml"
```

### Cover art support

In order to enable album cover support, user must set the [ImageBB API key](https://api.imgbb.com/) in the [configuration file](#configuration) by providing the key as below

```toml
[image.provider.imgbb]
api_key = "<YOUR API KEY>"
```

### Note

The application **must be restarted** after the configuration file is updated

## Autostarting

See [documentation](https://github.com/phusitsom/mprisence/wiki/Configuration) for autostarting.
