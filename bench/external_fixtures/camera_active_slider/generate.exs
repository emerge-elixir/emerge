alias Emerge.Engine
alias EmergeSkia.VideoTarget

output = __DIR__

state = Camera.ControlsController.init(%{}, %{})
controls = Camera.ControlsController.expose(state, %{}, %{})

events =
  Map.new(Camera.ControlsController.__events__(), fn event_name ->
    {event_name, {self(), {:criterion_fixture_event, event_name}}}
  end)

controls = Map.put(controls, :events_, events)

target = %VideoTarget{
  id: "camera-active-slider-benchmark",
  width: 1280,
  height: 720,
  mode: :prime,
  ref: make_ref()
}

preview = %{
  status: :live,
  video_target: target,
  stream_format: %{width: 1280, height: 720}
}

config = [
  display: [width: 2560, height: 1440, rotation: 90, ui_visible: true, video_visible: true],
  video: [width: 1280, height: 720],
  pipeline: [
    sensor_mode: {1920, 1080, 12},
    framerate: {60, 1},
    detection: [overlay: [enabled: false]]
  ]
]

Enum.each(0..7, fn phase ->
  requested = 10.0 + phase * 1.1
  actual = requested - 0.4

  phase_controls =
    controls
    |> Map.put(:shutter_ms, requested)
    |> put_in([:actual, :shutter_ms], actual)
    |> Map.put(:pending?, phase > 0)

  tree = Camera.UI.render(preview, phase_controls, nil, config)
  {encoded, _state, _assigned} = Engine.encode_full(Engine.diff_state_new(), tree)
  File.write!(Path.join(output, "phase_#{phase}.emrg"), encoded)
end)

IO.puts("wrote Camera active-slider fixtures to #{output}")
