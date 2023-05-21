# OBS Scene remote switcher

## Description

> This executable is made for bilibili live room: [5050](https://live.bilibili.com/5050) with love.

If you use dual machine for streaming (one is host, the other is streamer) and unfortunately you live in China, 
then you sometimes need to switch the obs from current scene to a *SAFE* scene to hide something that 超管 doesn't like.

> 如果直播中露出这样的画面，那直播生涯大概就要结束了叭

However, it is not easy to control the streaming machine when you are using the keyboard on the host machine, 
so this exe will enable you to use the global hotkey on host to control the scene in streamer.

## Requirements

OBS version >= 28 or install the [websocket plugin](https://github.com/obsproject/obs-websocket).

## Usage

1. Copy the config.example.toml to config.toml
2. Edit the config.toml
3. Launch the obs first
4. Launch the executable
5. Use the hotkey defined in config.toml to control obs
