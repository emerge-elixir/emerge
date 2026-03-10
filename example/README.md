# Emerge Demo

This example app renders a live WiFi broadcast H.265 pipeline into an
`EmergeSkia` `video/2` element and draws regular Emerge overlay UI on top.

It mirrors the same media chain as `WiFiPipeline2` from
`/workspace/gsn/lib/ground_station_nerves/wifi_pipeline.ex`, but swaps the DRM
sink for an `EmergeSkia` video target sink.

## What It Assumes

- Linux host
- `EmergeSkia` running on a real Wayland session for prime video import
- monitor-mode interfaces are already configured
- WiFi broadcast traffic is already present on those interfaces
- VAAPI decode is available on the configured render node

The example does not configure interfaces or start any external radio setup.

## Run

```bash
cd example
mix deps.get
mix run --no-halt
```

## Useful Environment Variables

- `EMERGE_DEMO_INTERFACES=wlan1,wlan2`
- `EMERGE_DEMO_KEY_PATH=/path/to/gs.key`
- `EMERGE_DEMO_LINK_ID=7669206`
- `EMERGE_DEMO_DECODER=/dev/dri/renderD129`
- `EMERGE_DEMO_BACKEND=wayland`
- `EMERGE_DEMO_WINDOW_WIDTH=1920`
- `EMERGE_DEMO_WINDOW_HEIGHT=1080`
- `EMERGE_DEMO_VIDEO_WIDTH=1920`
- `EMERGE_DEMO_VIDEO_HEIGHT=1080`

Defaults live in `example/config/config.exs`.

## Behavior

- Creates a fixed-size prime `EmergeSkia` video target
- Starts a Membrane pipeline using `Radio.Source -> Decrypt -> ReorderFec -> PayloadUnwrap -> RTP -> H265 -> PrimeDecoder`
- Submits `%Membrane.PrimeDesc{}` frames directly into the renderer target
- Shows a translucent overlay panel above the video so composition stays obvious
