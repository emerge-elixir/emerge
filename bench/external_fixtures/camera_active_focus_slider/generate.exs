alias Emerge.Engine

output = __DIR__

state = Camera.ControlsController.init(%{}, %{})
controls = Camera.ControlsController.expose(state, %{}, %{})

events =
  Map.new(Camera.ControlsController.__events__(), fn event_name ->
    {event_name, {self(), {:criterion_fixture_event, event_name}}}
  end)

controls = Map.put(controls, :events_, events)

target = :camera_active_focus_slider_benchmark

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
  requested = 0.00153 + phase * 0.5
  actual = Kernel.max(requested - 0.04, 0.00153)

  phase_controls =
    controls
    |> Map.put(:focus_diopters, requested)
    |> put_in([:actual, :focus_diopters], actual)
    |> Map.put(:pending?, phase > 0)

  tree = Camera.UI.render(preview, phase_controls, nil, config)
  {encoded, _state, _assigned} = Engine.encode_full(Engine.diff_state_new(), tree)
  File.write!(Path.join(output, "phase_#{phase}.emrg"), encoded)
end)

IO.puts("wrote Camera active-focus-slider fixtures to #{output}")
