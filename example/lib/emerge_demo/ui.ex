defmodule EmergeDemo.UI do
  @moduledoc false

  import Emerge.UI

  alias Emerge.UI.{Background, Border, Font}
  alias EmergeSkia.VideoTarget

  @ink {:color_rgba, {242, 245, 247, 255}}
  @muted {:color_rgba, {192, 202, 209, 255}}
  @panel {:color_rgba, {9, 13, 18, 214}}
  @panel_border {:color_rgba, {255, 166, 43, 170}}
  @accent_ok {:color_rgba, {61, 214, 140, 255}}
  @accent_warn {:color_rgba, {255, 166, 43, 255}}
  @accent_err {:color_rgba, {255, 92, 92, 255}}
  @accent_idle {:color_rgba, {122, 166, 255, 255}}

  @spec build_tree(VideoTarget.t(), map(), keyword()) :: Emerge.Element.t()
  def build_tree(%VideoTarget{} = target, runtime_state, config) do
    el(
      [
        width(fill()),
        height(fill()),
        Background.gradient({:color_rgb, {4, 6, 10}}, {:color_rgb, {12, 18, 28}}, 90),
        in_front(overlay_panel(runtime_state, config))
      ],
      video(target, [
        width(fill()),
        height(fill()),
        image_fit(:contain)
      ])
    )
  end

  defp overlay_panel(runtime_state, config) do
    column(
      [padding(20), spacing(14), width(fill())],
      [
        row([width(fill()), spacing(14)], [info_panel(runtime_state, config), status_badge(runtime_state)]),
        source_badge(config),
        footer_badge(runtime_state, config)
      ]
    )
  end

  defp info_panel(runtime_state, config) do
    renderer = Keyword.fetch!(config, :renderer)
    video = Keyword.fetch!(config, :video)
    pipeline = Keyword.fetch!(config, :pipeline)

    el(
      [
        width(px(410)),
        padding(18),
        Background.color(@panel),
        Border.rounded(20),
        Border.width(1),
        Border.color(@panel_border),
        Border.shadow(offset: {0, 14}, blur: 36, color: {:color_rgba, {0, 0, 0, 130}})
      ],
      column(
        [spacing(10)],
        [
          el([Font.size(14), Font.color(@accent_warn)], text("Membrane -> EmergeSkia")),
          el([Font.size(30), Font.color(@ink)], text("WiFi video preview")),
          el([Font.size(14), Font.color(@muted)], text(status_copy(runtime_state))),
          meta_line("Renderer", "#{renderer[:backend]} #{renderer[:width]}x#{renderer[:height]}"),
          meta_line("Target", "#{video[:mode]} #{video[:width]}x#{video[:height]}"),
          meta_line("Interfaces", Enum.join(pipeline[:interfaces], ", ")),
          meta_line("Decoder", pipeline[:decoder]),
          meta_line("Link ID", Integer.to_string(pipeline[:link_id]))
        ] ++ error_line(runtime_state)
      )
    )
  end

  defp status_badge(runtime_state) do
    {label, color} = status_badge_copy(runtime_state)

    el(
      [
        align_right(),
        padding(14),
        Background.color({:color_rgba, {6, 9, 13, 196}}),
        Border.rounded(999),
        Border.width(1),
        Border.color(color),
        Border.shadow(offset: {0, 10}, blur: 26, color: {:color_rgba, {0, 0, 0, 110}})
      ],
      column(
        [spacing(10)],
        [
          el([center_x(), Font.size(14), Font.color(color), Font.center()], text("LIVE")),
          el([Font.size(16), Font.color(@ink)], text(label))
        ]
      )
    )
  end

  defp source_badge(config) do
    pipeline = Keyword.fetch!(config, :pipeline)

    el(
      [
        padding(12),
        Background.color({:color_rgba, {8, 11, 17, 184}}),
        Border.rounded(16),
        Border.width(1),
        Border.color({:color_rgba, {122, 166, 255, 150}})
      ],
      el(
        [Font.size(14), Font.color(@muted)],
        text("Radio source: #{Enum.join(pipeline[:interfaces], ", ")}")
      )
    )
  end

  defp footer_badge(runtime_state, config) do
    video = Keyword.fetch!(config, :video)

    el(
      [
        padding(12),
        Background.color({:color_rgba, {8, 11, 17, 184}}),
        Border.rounded(16),
        Border.width(1),
        Border.color({:color_rgba, {255, 166, 43, 150}})
      ],
      el(
        [Font.size(14), Font.color(@muted)],
        text(
          "Mode #{video[:mode]} | #{video[:width]}x#{video[:height]} | #{stream_copy(runtime_state)}"
        )
      )
    )
  end

  defp meta_line(label, value) do
    el(
      [Font.size(15), Font.color(@muted)],
      text(label <> ": " <> value)
    )
  end

  defp error_line(%{last_error: nil}), do: []

  defp error_line(%{last_error: error}) do
    [
      el([Font.size(14), Font.color(@accent_err)], text("Error: #{error}"))
    ]
  end

  defp status_copy(%{status: :booting}), do: "Booting renderer and preparing the preview surface."

  defp status_copy(%{status: :starting_pipeline}),
    do: "Renderer is live. Starting the WiFi Membrane pipeline."

  defp status_copy(%{status: :pipeline_playing}),
    do: "Pipeline is running. Waiting for the first decoded prime frame."

  defp status_copy(%{status: :waiting_for_frames}),
    do: "The sink is ready; the UI will swap to live video as soon as frames arrive."

  defp status_copy(%{status: :live}),
    do: "Prime frames are landing directly in the renderer video target with overlays on top."

  defp status_copy(%{status: :pipeline_down}),
    do: "The pipeline stopped. Inspect the error line below for the exit reason."

  defp status_copy(_runtime_state), do: "Preparing demo state."

  defp status_badge_copy(%{status: :live}), do: {"Prime frames flowing", @accent_ok}
  defp status_badge_copy(%{status: :pipeline_down}), do: {"Pipeline stopped", @accent_err}
  defp status_badge_copy(%{status: :waiting_for_frames}), do: {"Waiting for frames", @accent_warn}
  defp status_badge_copy(%{status: :pipeline_playing}), do: {"Pipeline playing", @accent_idle}
  defp status_badge_copy(%{status: :starting_pipeline}), do: {"Starting pipeline", @accent_idle}
  defp status_badge_copy(_runtime_state), do: {"Booting demo", @accent_idle}

  defp stream_copy(%{stream_format: nil}), do: "awaiting stream format"

  defp stream_copy(%{stream_format: %{width: width, height: height, framerate: {num, den}}}) do
    "#{width}x#{height} @ #{num}/#{den}"
  end
end
