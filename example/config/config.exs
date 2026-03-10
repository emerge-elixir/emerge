import Config

config :logger, level: :info

env_integer = fn name, default ->
  case System.get_env(name) do
    nil -> default
    value -> String.to_integer(value)
  end
end

env_string = fn name, default ->
  case System.get_env(name) do
    nil -> default
    value -> value
  end
end

env_list = fn name, default ->
  case System.get_env(name) do
    nil ->
      default

    value ->
      value
      |> String.split(",", trim: true)
      |> Enum.map(&String.trim/1)
      |> Enum.reject(&(&1 == ""))
  end
end

default_key_path = Path.expand("../../../nerves-wifibroadcast/gs.key", __DIR__)

config :emerge_demo, EmergeDemo.Application, auto_start?: true

config :emerge_demo, EmergeDemo.Runtime,
  renderer: [
    backend: env_string.("EMERGE_DEMO_BACKEND", "wayland"),
    drm_card: env_string.("EMERGE_DEMO_BACKEND", "/dev/dri/card0"),
    title: env_string.("EMERGE_DEMO_TITLE", "Emerge WiFi Video Demo"),
    width: env_integer.("EMERGE_DEMO_WINDOW_WIDTH", 1920),
    height: env_integer.("EMERGE_DEMO_WINDOW_HEIGHT", 1080)
  ],
  video: [
    id: env_string.("EMERGE_DEMO_VIDEO_ID", "wifi-preview"),
    width: env_integer.("EMERGE_DEMO_VIDEO_WIDTH", 1920),
    height: env_integer.("EMERGE_DEMO_VIDEO_HEIGHT", 1080),
    mode: :prime
  ],
  pipeline: [
    interfaces: env_list.("EMERGE_DEMO_INTERFACES", ["wlan0", "wlan1"]),
    key_path: env_string.("EMERGE_DEMO_KEY_PATH", default_key_path),
    link_id: env_integer.("EMERGE_DEMO_LINK_ID", 7_669_206),
    decoder: env_string.("EMERGE_DEMO_DECODER", "/dev/dri/renderD128"),
    framerate: {60, 1}
  ],
  renderer_check_interval_ms: env_integer.("EMERGE_DEMO_RENDERER_CHECK_MS", 500)

import_config "#{config_env()}.exs"
