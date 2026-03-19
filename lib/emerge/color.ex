defmodule Emerge.Color do
  @moduledoc """
  Helpers for UI color tuples, including the Tailwind CSS v4.2 palette.

  These helpers return the tuple formats accepted by `Emerge.UI` color
  attributes such as `Background.color/1`, `Border.color/1`, `Font.color/1`,
  and `Svg.color/1`.

  Tailwind palette values are derived from the official v4.2 OKLCH tokens and
  converted to stable sRGB byte tuples.

  ## Examples

      iex> Emerge.Color.color(:sky)
      {:color_rgb, {0, 188, 255}}

      iex> Emerge.Color.color(:rose, 300)
      {:color_rgb, {255, 161, 173}}

      iex> Emerge.Color.color(:sky, 200, 0.3)
      {:color_rgba, {184, 230, 254, 77}}

      iex> Emerge.Color.color_rgba(12, 34, 56, 0.5)
      {:color_rgba, {12, 34, 56, 128}}
  """

  @typedoc "A single RGB or alpha byte channel."
  @type channel :: 0..255

  @typedoc "Supported Tailwind shade values."
  @type shade :: 50 | 100 | 200 | 300 | 400 | 500 | 600 | 700 | 800 | 900 | 950

  @typedoc "Tuple accepted by `Emerge.UI` color attributes."
  @type rgb_tuple :: {:color_rgb, {channel(), channel(), channel()}}
  @type rgba_tuple :: {:color_rgba, {channel(), channel(), channel(), channel()}}
  @type t :: rgb_tuple() | rgba_tuple()

  @default_shade 400
  @shades [50, 100, 200, 300, 400, 500, 600, 700, 800, 900, 950]

  @palette_entries [
    slate: [
      {50, {248, 250, 252}},
      {100, {241, 245, 249}},
      {200, {226, 232, 240}},
      {300, {202, 213, 226}},
      {400, {144, 161, 185}},
      {500, {98, 116, 142}},
      {600, {69, 85, 108}},
      {700, {49, 65, 88}},
      {800, {29, 41, 61}},
      {900, {15, 23, 43}},
      {950, {2, 6, 24}}
    ],
    gray: [
      {50, {249, 250, 251}},
      {100, {243, 244, 246}},
      {200, {229, 231, 235}},
      {300, {209, 213, 220}},
      {400, {153, 161, 175}},
      {500, {106, 114, 130}},
      {600, {74, 85, 101}},
      {700, {54, 65, 83}},
      {800, {30, 41, 57}},
      {900, {16, 24, 40}},
      {950, {3, 7, 18}}
    ],
    zinc: [
      {50, {250, 250, 250}},
      {100, {244, 244, 245}},
      {200, {228, 228, 231}},
      {300, {212, 212, 216}},
      {400, {159, 159, 169}},
      {500, {113, 113, 123}},
      {600, {82, 82, 92}},
      {700, {63, 63, 70}},
      {800, {39, 39, 42}},
      {900, {24, 24, 27}},
      {950, {9, 9, 11}}
    ],
    neutral: [
      {50, {250, 250, 250}},
      {100, {245, 245, 245}},
      {200, {229, 229, 229}},
      {300, {212, 212, 212}},
      {400, {161, 161, 161}},
      {500, {115, 115, 115}},
      {600, {82, 82, 82}},
      {700, {64, 64, 64}},
      {800, {38, 38, 38}},
      {900, {23, 23, 23}},
      {950, {10, 10, 10}}
    ],
    stone: [
      {50, {250, 250, 249}},
      {100, {245, 245, 244}},
      {200, {231, 229, 228}},
      {300, {214, 211, 209}},
      {400, {166, 160, 155}},
      {500, {121, 113, 107}},
      {600, {87, 83, 77}},
      {700, {68, 64, 59}},
      {800, {41, 37, 36}},
      {900, {28, 25, 23}},
      {950, {12, 10, 9}}
    ],
    red: [
      {50, {254, 242, 242}},
      {100, {255, 226, 226}},
      {200, {255, 201, 201}},
      {300, {255, 162, 162}},
      {400, {255, 100, 103}},
      {500, {251, 44, 54}},
      {600, {231, 0, 11}},
      {700, {193, 0, 7}},
      {800, {159, 7, 18}},
      {900, {130, 24, 26}},
      {950, {70, 8, 9}}
    ],
    orange: [
      {50, {255, 247, 237}},
      {100, {255, 237, 212}},
      {200, {255, 214, 167}},
      {300, {255, 184, 106}},
      {400, {255, 137, 4}},
      {500, {255, 105, 0}},
      {600, {245, 73, 0}},
      {700, {202, 53, 0}},
      {800, {159, 45, 0}},
      {900, {126, 42, 12}},
      {950, {68, 19, 6}}
    ],
    amber: [
      {50, {255, 251, 235}},
      {100, {254, 243, 198}},
      {200, {254, 230, 133}},
      {300, {255, 210, 48}},
      {400, {255, 185, 0}},
      {500, {254, 154, 0}},
      {600, {225, 113, 0}},
      {700, {187, 77, 0}},
      {800, {151, 60, 0}},
      {900, {123, 51, 6}},
      {950, {70, 25, 1}}
    ],
    yellow: [
      {50, {254, 252, 232}},
      {100, {254, 249, 194}},
      {200, {255, 240, 133}},
      {300, {255, 223, 32}},
      {400, {253, 199, 0}},
      {500, {240, 177, 0}},
      {600, {208, 135, 0}},
      {700, {166, 95, 0}},
      {800, {137, 75, 0}},
      {900, {115, 62, 10}},
      {950, {67, 32, 4}}
    ],
    lime: [
      {50, {247, 254, 231}},
      {100, {236, 252, 202}},
      {200, {216, 249, 153}},
      {300, {187, 244, 81}},
      {400, {154, 230, 0}},
      {500, {124, 207, 0}},
      {600, {94, 165, 0}},
      {700, {73, 125, 0}},
      {800, {60, 99, 0}},
      {900, {53, 83, 14}},
      {950, {25, 46, 3}}
    ],
    green: [
      {50, {240, 253, 244}},
      {100, {220, 252, 231}},
      {200, {185, 248, 207}},
      {300, {123, 241, 168}},
      {400, {5, 223, 114}},
      {500, {0, 201, 80}},
      {600, {0, 166, 62}},
      {700, {0, 130, 54}},
      {800, {1, 102, 48}},
      {900, {13, 84, 43}},
      {950, {3, 46, 21}}
    ],
    emerald: [
      {50, {236, 253, 245}},
      {100, {208, 250, 229}},
      {200, {164, 244, 207}},
      {300, {94, 233, 181}},
      {400, {0, 212, 146}},
      {500, {0, 188, 125}},
      {600, {0, 153, 102}},
      {700, {0, 122, 85}},
      {800, {0, 96, 69}},
      {900, {0, 79, 59}},
      {950, {0, 44, 34}}
    ],
    teal: [
      {50, {240, 253, 250}},
      {100, {203, 251, 241}},
      {200, {150, 247, 228}},
      {300, {70, 236, 213}},
      {400, {0, 213, 190}},
      {500, {0, 187, 167}},
      {600, {0, 150, 137}},
      {700, {0, 120, 111}},
      {800, {0, 95, 90}},
      {900, {11, 79, 74}},
      {950, {2, 47, 46}}
    ],
    cyan: [
      {50, {236, 254, 255}},
      {100, {206, 250, 254}},
      {200, {162, 244, 253}},
      {300, {83, 234, 253}},
      {400, {0, 211, 242}},
      {500, {0, 184, 219}},
      {600, {0, 146, 184}},
      {700, {0, 117, 149}},
      {800, {0, 95, 120}},
      {900, {16, 78, 100}},
      {950, {5, 51, 69}}
    ],
    sky: [
      {50, {240, 249, 255}},
      {100, {223, 242, 254}},
      {200, {184, 230, 254}},
      {300, {116, 212, 255}},
      {400, {0, 188, 255}},
      {500, {0, 166, 244}},
      {600, {0, 132, 209}},
      {700, {0, 105, 168}},
      {800, {0, 89, 138}},
      {900, {2, 74, 112}},
      {950, {5, 47, 74}}
    ],
    blue: [
      {50, {239, 246, 255}},
      {100, {219, 234, 254}},
      {200, {190, 219, 255}},
      {300, {142, 197, 255}},
      {400, {81, 162, 255}},
      {500, {43, 127, 255}},
      {600, {21, 93, 252}},
      {700, {20, 71, 230}},
      {800, {25, 60, 184}},
      {900, {28, 57, 142}},
      {950, {22, 36, 86}}
    ],
    indigo: [
      {50, {238, 242, 255}},
      {100, {224, 231, 255}},
      {200, {198, 210, 255}},
      {300, {163, 179, 255}},
      {400, {124, 134, 255}},
      {500, {97, 95, 255}},
      {600, {79, 57, 246}},
      {700, {67, 45, 215}},
      {800, {55, 42, 172}},
      {900, {49, 44, 133}},
      {950, {30, 26, 77}}
    ],
    violet: [
      {50, {245, 243, 255}},
      {100, {237, 233, 254}},
      {200, {221, 214, 255}},
      {300, {196, 180, 255}},
      {400, {166, 132, 255}},
      {500, {142, 81, 255}},
      {600, {127, 34, 254}},
      {700, {112, 8, 231}},
      {800, {93, 14, 192}},
      {900, {77, 23, 154}},
      {950, {47, 13, 104}}
    ],
    purple: [
      {50, {250, 245, 255}},
      {100, {243, 232, 255}},
      {200, {233, 212, 255}},
      {300, {218, 178, 255}},
      {400, {194, 122, 255}},
      {500, {173, 70, 255}},
      {600, {152, 16, 250}},
      {700, {130, 0, 219}},
      {800, {110, 17, 176}},
      {900, {89, 22, 139}},
      {950, {60, 3, 102}}
    ],
    fuchsia: [
      {50, {253, 244, 255}},
      {100, {250, 232, 255}},
      {200, {246, 207, 255}},
      {300, {244, 168, 255}},
      {400, {237, 106, 255}},
      {500, {225, 42, 251}},
      {600, {200, 0, 222}},
      {700, {168, 0, 183}},
      {800, {138, 1, 148}},
      {900, {114, 19, 120}},
      {950, {75, 0, 79}}
    ],
    pink: [
      {50, {253, 242, 248}},
      {100, {252, 231, 243}},
      {200, {252, 206, 232}},
      {300, {253, 165, 213}},
      {400, {251, 100, 182}},
      {500, {246, 51, 154}},
      {600, {230, 0, 118}},
      {700, {198, 0, 92}},
      {800, {163, 0, 76}},
      {900, {134, 16, 67}},
      {950, {81, 4, 36}}
    ],
    rose: [
      {50, {255, 241, 242}},
      {100, {255, 228, 230}},
      {200, {255, 204, 211}},
      {300, {255, 161, 173}},
      {400, {255, 99, 126}},
      {500, {255, 32, 86}},
      {600, {236, 0, 63}},
      {700, {199, 0, 54}},
      {800, {165, 0, 54}},
      {900, {139, 8, 54}},
      {950, {77, 2, 24}}
    ],
    taupe: [
      {50, {251, 250, 249}},
      {100, {243, 241, 241}},
      {200, {232, 228, 227}},
      {300, {216, 210, 208}},
      {400, {171, 160, 156}},
      {500, {124, 109, 103}},
      {600, {91, 79, 75}},
      {700, {71, 60, 57}},
      {800, {43, 36, 34}},
      {900, {29, 24, 22}},
      {950, {12, 10, 9}}
    ],
    mauve: [
      {50, {250, 250, 250}},
      {100, {243, 241, 243}},
      {200, {231, 228, 231}},
      {300, {215, 208, 215}},
      {400, {168, 158, 169}},
      {500, {121, 105, 123}},
      {600, {89, 76, 91}},
      {700, {70, 57, 71}},
      {800, {42, 33, 44}},
      {900, {29, 22, 30}},
      {950, {12, 9, 12}}
    ],
    mist: [
      {50, {249, 251, 251}},
      {100, {241, 243, 243}},
      {200, {227, 231, 232}},
      {300, {208, 214, 216}},
      {400, {156, 168, 171}},
      {500, {103, 120, 124}},
      {600, {75, 88, 91}},
      {700, {57, 68, 71}},
      {800, {34, 41, 43}},
      {900, {22, 27, 29}},
      {950, {9, 11, 12}}
    ],
    olive: [
      {50, {251, 251, 249}},
      {100, {244, 244, 240}},
      {200, {232, 232, 227}},
      {300, {216, 216, 208}},
      {400, {171, 171, 156}},
      {500, {124, 124, 103}},
      {600, {91, 91, 75}},
      {700, {71, 71, 57}},
      {800, {43, 43, 34}},
      {900, {29, 29, 22}},
      {950, {12, 12, 9}}
    ]
  ]

  @flat_color_entries [black: {0, 0, 0}, white: {255, 255, 255}]
  @palette_names Keyword.keys(@palette_entries)
  @flat_color_names Keyword.keys(@flat_color_entries)
  @all_color_names @palette_names ++ @flat_color_names

  @palette @palette_entries
           |> Enum.map(fn {name, shades} -> {name, Map.new(shades)} end)
           |> Map.new()

  @flat_colors Map.new(@flat_color_entries)
  @supported_names_message Enum.map_join(@all_color_names, ", ", &inspect/1)
  @supported_shades_message Enum.map_join(@shades, ", ", &Integer.to_string/1)

  @doc """
  Return a Tailwind or flat color tuple.

  `shade` defaults to `#{@default_shade}` and `alpha` uses CSS-style fractional
  opacity from `0.0` to `1.0`.

  Opaque colors collapse to `{:color_rgb, {r, g, b}}`. Translucent colors return
  `{:color_rgba, {r, g, b, a}}`.

  ## Examples

      iex> Emerge.Color.color(:sky)
      {:color_rgb, {0, 188, 255}}

      iex> Emerge.Color.color(:sky, 200, 0.3)
      {:color_rgba, {184, 230, 254, 77}}

      iex> Emerge.Color.color(:black, 400, 0.5)
      {:color_rgba, {0, 0, 0, 128}}
  """
  @spec color(atom(), integer(), number()) :: t()
  def color(name, shade \\ @default_shade, alpha \\ 1.0) do
    name = validate_name!(name)
    shade = validate_shade!(shade)
    rgb = rgb_for!(name, shade)
    alpha_byte = alpha_byte!(alpha, "color/3")

    build_color_tuple(rgb, alpha_byte, collapse_opaque?: true)
  end

  @doc """
  Return a raw RGB tuple accepted by `Emerge.UI` color attributes.

  ## Example

      iex> Emerge.Color.color_rgb(12, 34, 56)
      {:color_rgb, {12, 34, 56}}
  """
  @spec color_rgb(integer(), integer(), integer()) :: rgb_tuple()
  def color_rgb(r, g, b) do
    {:color_rgb,
     {validate_channel!(r, :r, "color_rgb/3"), validate_channel!(g, :g, "color_rgb/3"),
      validate_channel!(b, :b, "color_rgb/3")}}
  end

  @doc """
  Return a raw RGBA tuple accepted by `Emerge.UI` color attributes.

  `alpha` uses CSS-style fractional opacity from `0.0` to `1.0`.

  ## Example

      iex> Emerge.Color.color_rgba(12, 34, 56, 0.5)
      {:color_rgba, {12, 34, 56, 128}}
  """
  @spec color_rgba(integer(), integer(), integer(), number()) :: rgba_tuple()
  def color_rgba(r, g, b, alpha) do
    {:color_rgba,
     {validate_channel!(r, :r, "color_rgba/4"), validate_channel!(g, :g, "color_rgba/4"),
      validate_channel!(b, :b, "color_rgba/4"), alpha_byte!(alpha, "color_rgba/4")}}
  end

  defp validate_name!(name) when is_atom(name) and name in @all_color_names, do: name

  defp validate_name!(name) do
    raise ArgumentError,
          "unknown color name #{inspect(name)}; supported names: #{@supported_names_message}"
  end

  defp validate_shade!(shade) when is_integer(shade), do: shade

  defp validate_shade!(shade) do
    raise ArgumentError,
          "shade must be an integer; supported shades: #{@supported_shades_message}, got: #{inspect(shade)}"
  end

  defp rgb_for!(name, shade) when name in @flat_color_names do
    if shade == @default_shade do
      Map.fetch!(@flat_colors, name)
    else
      raise ArgumentError,
            "#{inspect(name)} does not support shade #{inspect(shade)}; use shade #{@default_shade} or omit the shade"
    end
  end

  defp rgb_for!(name, shade) do
    palette = Map.fetch!(@palette, name)

    case Map.fetch(palette, shade) do
      {:ok, rgb} ->
        rgb

      :error ->
        raise ArgumentError,
              "unknown shade #{inspect(shade)} for #{inspect(name)}; supported shades: #{@supported_shades_message}"
    end
  end

  defp validate_channel!(value, _channel, _caller)
       when is_integer(value) and value >= 0 and value <= 255,
       do: value

  defp validate_channel!(value, channel, caller) do
    raise ArgumentError,
          "#{caller} expects #{channel} to be an integer between 0 and 255, got: #{inspect(value)}"
  end

  defp alpha_byte!(alpha, caller) when is_number(alpha) do
    alpha = alpha * 1.0

    if alpha < 0.0 or alpha > 1.0 do
      raise ArgumentError,
            "#{caller} expects alpha to be between 0.0 and 1.0, got: #{inspect(alpha)}"
    end

    alpha
    |> min(1.0)
    |> max(0.0)
    |> Kernel.*(255)
    |> round()
  end

  defp alpha_byte!(alpha, caller) do
    raise ArgumentError,
          "#{caller} expects alpha to be between 0.0 and 1.0, got: #{inspect(alpha)}"
  end

  defp build_color_tuple({r, g, b}, 255, collapse_opaque?: true), do: {:color_rgb, {r, g, b}}

  defp build_color_tuple({r, g, b}, alpha, collapse_opaque?: _collapse?),
    do: {:color_rgba, {r, g, b, alpha}}
end
