# mprisence

|           | Playing                                                   | Paused                                                  |
| --------- | --------------------------------------------------------- | ------------------------------------------------------- |
| No icon   | ![Playing, No icon](assets/readme/playing-noicon.png)     | ![Paused, No icon](assets/readme/paused-noicon.png)     |
| Show icon | ![Playing, Show icon](assets/readme/playing-showicon.png) | ![Paused, Show icon](assets/readme/paused-showicon.png) |
| No cover  | ![Playing, No cover](assets/readme/playing-nocover.png)   | ![Paused, No cover](assets/readme/paused-nocover.png)   |

A Discord Rich Presence client for MPRIS-compatible media players with album/song cover art support

## Installation

```bash
bash <(curl https://raw.githubusercontent.com/phusitsom/mprisence/main/scripts/install.sh)
```

To enable **cover art support**, [see below](#cover-art-support).

## Usage

To start mprisence, simply run this command:

```bash
mprisence
```

## Configuration

The rich presence can be configured to the user's preference by providing the configuration file at `~/.config/mprisence/config.toml` or `$XDG_CONFIG_HOME/mprisence/config.toml`.

See [Example config file](config/example.toml) for more detail on configuration.

To download example config file:

```bash
bash <(curl https://raw.githubusercontent.com/phusitsom/mprisence/main/scripts/example-config.sh)
```

See also:

- [Default config file](config/default.toml)

### Cover art support

In order to enable album cover support, user must set the [ImageBB API key](https://api.imgbb.com/) in the [configuration file](#configuration) by providing the key as below

```toml
[image.provider.imgbb]
api_key = "<YOUR API KEY>"
```

### Note

The application **must be restarted** after the configuration file is updated

## Autostarting

For most Linux distributions, you can use [systemd](https://wiki.archlinux.org/title/Systemd) to autostart mprisence.

```bash
systemctl --user enable --now mprisence.service
```

If the configuration file is updated, you must restart the service

```bash
systemctl --user restart mprisence.service
```
