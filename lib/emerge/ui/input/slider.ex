defmodule Emerge.UI.Input.Slider do
  @moduledoc """
  Configuration helpers for `Emerge.UI.Input.slider/2`.

  `Slider.config/1` is an attribute-like value that only `Input.slider/2`
  accepts. It configures the numeric range and optional visual slots.
  """

  alias Emerge.Engine.Element
  alias Emerge.UI.Internal.Builder
  alias Emerge.UI.Internal.Validation

  @allowed_keys [:min, :max, :step, :track, :filled_track, :thumb]
  @default_min 0.0
  @default_max 1.0
  @default_step 0.0

  @type t :: {:slider_config, map()}

  @doc """
  Configure an `Input.slider/2`.

  Supported options:

  - `:min` and `:max` set the numeric range.
  - `:step` sets snapping. Use `nil`, `:any`, or omit it for continuous values.
  - `:track`, `:filled_track`, and `:thumb` accept regular Emerge elements.

  The slider owns track widths, so the `:track` and `:filled_track` root
  elements must not set `width(...)`.
  """
  @spec config(keyword()) :: t()
  def config(opts \\ [])

  def config(opts) when is_list(opts) do
    unknown = Keyword.keys(opts) -- @allowed_keys

    if unknown != [] do
      raise ArgumentError,
            "Slider.config/1 does not support option #{inspect(hd(unknown))}; " <>
              "supported options are #{inspect(@allowed_keys)}"
    end

    {:slider_config, Map.new(opts)}
  end

  def config(other) do
    raise ArgumentError, "Slider.config/1 expects a keyword list, got: #{inspect(other)}"
  end

  @doc false
  @spec normalize_config!(String.t(), map(), number()) :: %{
          min: float(),
          max: float(),
          step: float(),
          value: float(),
          track: Element.t(),
          filled_track: Element.t(),
          thumb: Element.t()
        }
  def normalize_config!(owner, config, value) when is_map(config) do
    min = normalize_number!(owner, :min, Map.get(config, :min, @default_min))
    max = normalize_number!(owner, :max, Map.get(config, :max, @default_max))

    if max <= min do
      raise ArgumentError,
            "#{owner} expects Slider.config/1 :max to be greater than :min, got min: #{inspect(min)}, max: #{inspect(max)}"
    end

    step =
      case Map.get(config, :step, @default_step) do
        nil -> @default_step
        :any -> @default_step
        step -> normalize_number!(owner, :step, step)
      end

    if step < 0 do
      raise ArgumentError,
            "#{owner} expects Slider.config/1 :step to be zero, positive, or nil, got: #{inspect(step)}"
    end

    track = Map.get(config, :track, default_track())
    filled_track = Map.get(config, :filled_track, default_filled_track())
    thumb = Map.get(config, :thumb, default_thumb())

    track = validate_slot!(owner, :track, track)
    filled_track = validate_slot!(owner, :filled_track, filled_track)
    thumb = validate_slot!(owner, :thumb, thumb)

    validate_track_width_absent!(owner, :track, track)
    validate_track_width_absent!(owner, :filled_track, filled_track)

    value =
      owner
      |> normalize_number!(:value, value)
      |> clamp(min, max)
      |> snap(min, max, step)

    %{
      min: min,
      max: max,
      step: step,
      value: value,
      track: track,
      filled_track: filled_track,
      thumb: thumb
    }
  end

  defp normalize_number!(_owner, _key, value) when is_number(value), do: value * 1.0

  defp normalize_number!(owner, key, value) do
    label =
      case key do
        :value -> "second argument"
        key -> "Slider.config/1 #{inspect(key)}"
      end

    raise ArgumentError, "#{owner} expects #{label} to be a number, got: #{inspect(value)}"
  end

  defp validate_slot!(owner, slot, value) do
    Validation.validate_child_element!("#{owner} Slider.config/1 #{inspect(slot)}", value)
  end

  defp validate_track_width_absent!(owner, slot, %Element{attrs: attrs}) do
    if Map.has_key?(attrs, :width) do
      raise ArgumentError,
            "#{owner} does not allow Slider.config/1 #{inspect(slot)} to set width; the slider owns track width"
    end
  end

  defp clamp(value, min, max), do: value |> Kernel.max(min) |> Kernel.min(max)

  defp snap(value, _min, _max, step) when step <= 0, do: value

  defp snap(value, min, max, step) do
    units = round((value - min) / step)
    clamp(min + units * step, min, max)
  end

  defp default_track do
    Builder.build_element(
      %{
        height: {:px, 4},
        background: {:color_rgb, {226, 232, 240}},
        border_radius: 2
      },
      :el,
      [none()]
    )
  end

  defp default_filled_track do
    Builder.build_element(
      %{
        height: {:px, 4},
        background: {:color_rgb, {14, 165, 233}},
        border_radius: 2
      },
      :el,
      [none()]
    )
  end

  defp default_thumb do
    Builder.build_element(
      %{
        width: {:px, 18},
        height: {:px, 18},
        background: {:color_rgb, {255, 255, 255}},
        border_radius: 9,
        border_width: 1,
        border_color: {:color_rgb, {14, 165, 233}}
      },
      :el,
      [none()]
    )
  end

  defp none, do: Builder.build_element(%{}, :none, [])
end
