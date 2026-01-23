defmodule EmergeSkia.EmrgRoundtripTest do
  use ExUnit.Case

  import Emerge.UI

  defp normalize_tree(%Emerge.Element{} = element) do
    %{
      type: element.type,
      id: element.id,
      attrs: normalize_attrs(element.attrs),
      children: Enum.map(element.children, &normalize_tree/1)
    }
  end

  defp normalize_attrs(attrs) do
    attrs
    |> Emerge.Tree.strip_runtime_attrs()
    |> Enum.map(fn {key, value} -> {key, normalize_attr_value(value)} end)
    |> Map.new()
  end

  defp normalize_attr_value(%Emerge.Element{} = element), do: normalize_tree(element)
  defp normalize_attr_value(%{type: type, id: id, attrs: attrs, children: children}) do
    %{
      type: type,
      id: id,
      attrs: normalize_attrs(attrs),
      children: Enum.map(children, &normalize_attr_value/1)
    }
  end
  defp normalize_attr_value(value), do: value

  test "EMRG roundtrip through Rust preserves tree" do
    tree =
      column(
        [
          width(:fill),
          height(:fill),
          padding(20.0),
          spacing(12.0),
          Emerge.UI.Background.color(:black),
          {:snap_layout, true}
        ],
        [
          el(
            [
              padding(10.0),
              width({:px, 240.0}),
              height(:content),
              Emerge.UI.Background.gradient(
                {:color_rgba, {20, 30, 40, 255}},
                {:color_rgba, {60, 70, 80, 255}},
                45.0
              ),
              Emerge.UI.Border.rounded_each(8.0, 6.0, 4.0, 2.0),
              Emerge.UI.Border.width(2.0),
              Emerge.UI.Border.color(:white),
              {:snap_text_metrics, true}
            ],
            el(
              [
                Emerge.UI.Font.size(18.0),
                Emerge.UI.Font.color(:white),
                Emerge.UI.Font.family(:serif),
                Emerge.UI.Font.bold,
                Emerge.UI.Font.italic
              ],
              text("Hello")
            )
          ),
          row([spacing(8.0), align_top(), width({:fill_portion, 2.0})], [
            el([padding(6.0), Emerge.UI.Background.color(:red), Emerge.UI.Font.size(12.0), Emerge.UI.Font.color(:white)], text("A")),
            el([padding(6.0), Emerge.UI.Background.color(:green), Emerge.UI.Font.size(12.0), Emerge.UI.Font.color(:white)], text("B")),
            el([padding(6.0), Emerge.UI.Background.color(:blue), Emerge.UI.Font.size(12.0), Emerge.UI.Font.color(:white)], text("C"))
          ]),
          el(
            [
              padding(8.0),
              width(:content),
              height({:px, 32.0}),
              Emerge.UI.Background.color({:color_rgb, {10, 20, 30}}),
              Emerge.UI.Border.rounded(6.0),
              scrollbar_x(),
              scrollbar_y(),
              clip(),
              clip_x(),
              clip_y()
            ],
            el([Emerge.UI.Font.size(14.0), Emerge.UI.Font.color(:white)], text("Clipped"))
          ),
          el(
            [
              padding_each(2.0, 4.0, 6.0, 8.0),
              width({:px, 160.0}),
              height({:px, 36.0}),
              align_left(),
              align_bottom(),
              Emerge.UI.Background.color({:color_rgba, {12, 34, 56, 78}}),
              Emerge.UI.Border.color({:color_rgb, {200, 210, 220}})
            ],
            el([Emerge.UI.Font.size(12.0), Emerge.UI.Font.color(:white)], text("Align + tuple pad"))
          ),
          el(
            [
              {:padding, %{top: 1.0, right: 3.0, bottom: 5.0, left: 7.0}},
              width({:fill_portion, 1.0}),
              height(:content),
              align_right(),
              center_y(),
              Emerge.UI.Background.color(:gray),
              Emerge.UI.Border.rounded_each(4.0, 4.0, 4.0, 4.0)
            ],
            el([Emerge.UI.Font.size(11.0), Emerge.UI.Font.color(:white)], text("Map padding"))
          ),
          el(
            [
              padding(4.0),
              center_x(),
              center_y(),
              above(el([padding(2.0), Emerge.UI.Background.color(:gray), Emerge.UI.Font.size(10.0), Emerge.UI.Font.color(:white)], text("Above"))),
              below(el([padding(2.0), Emerge.UI.Background.color(:gray), Emerge.UI.Font.size(10.0), Emerge.UI.Font.color(:white)], text("Below"))),
              on_left(el([padding(2.0), Emerge.UI.Background.color(:gray), Emerge.UI.Font.size(10.0), Emerge.UI.Font.color(:white)], text("Left"))),
              on_right(el([padding(2.0), Emerge.UI.Background.color(:gray), Emerge.UI.Font.size(10.0), Emerge.UI.Font.color(:white)], text("Right"))),
              in_front(el([padding(2.0), Emerge.UI.Background.color(:gray), Emerge.UI.Font.size(10.0), Emerge.UI.Font.color(:white)], text("Front"))),
              behind_content(el([padding(2.0), Emerge.UI.Background.color(:gray), Emerge.UI.Font.size(10.0), Emerge.UI.Font.color(:white)], text("Behind")))
            ],
            el(
              [Emerge.UI.Font.size(16.0), Emerge.UI.Font.color(:white), Emerge.UI.Font.family("Fira Sans")],
              text("Nearby")
            )
          )
        ]
      )

    {_vdom, assigned} = Emerge.Reconcile.assign_ids(tree)
    encoded = Emerge.Serialization.encode_tree(assigned)

    roundtrip =
      case EmergeSkia.Native.tree_roundtrip(encoded) do
        bin when is_binary(bin) -> bin
        {:ok, bin} when is_binary(bin) -> bin
        {:error, reason} -> flunk("tree_roundtrip failed: #{reason}")
        other -> flunk("unexpected tree_roundtrip result: #{inspect(other)}")
      end

    decoded = Emerge.Serialization.decode(roundtrip)

    normalized_decoded = normalize_tree(decoded)
    normalized_assigned = normalize_tree(assigned)

    assert normalized_decoded == normalized_assigned
  end

  test "EMRG attribute encoding is order-stable" do
    attrs_a = %{
      width: :fill,
      height: :content,
      padding: 10.0,
      font_size: 12.0,
      font_color: :white,
      border_radius: 4.0
    }

    attrs_b = %{
      border_radius: 4.0,
      font_color: :white,
      font_size: 12.0,
      padding: 10.0,
      height: :content,
      width: :fill
    }

    encoded_a = Emerge.AttrCodec.encode_attrs(attrs_a)
    encoded_b = Emerge.AttrCodec.encode_attrs(attrs_b)

    assert encoded_a == encoded_b
  end
end
