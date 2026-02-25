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
                Emerge.UI.Font.bold(),
                Emerge.UI.Font.italic()
              ],
              text("Hello")
            )
          ),
          row([spacing(8.0), align_top(), width({:fill_portion, 2.0})], [
            el(
              [
                padding(6.0),
                Emerge.UI.Background.color(:red),
                Emerge.UI.Font.size(12.0),
                Emerge.UI.Font.color(:white)
              ],
              text("A")
            ),
            el(
              [
                padding(6.0),
                Emerge.UI.Background.color(:green),
                Emerge.UI.Font.size(12.0),
                Emerge.UI.Font.color(:white)
              ],
              text("B")
            ),
            el(
              [
                padding(6.0),
                Emerge.UI.Background.color(:blue),
                Emerge.UI.Font.size(12.0),
                Emerge.UI.Font.color(:white)
              ],
              text("C")
            )
          ]),
          el(
            [
              padding(8.0),
              width(:content),
              height({:px, 32.0}),
              Emerge.UI.Background.color({:color_rgb, {10, 20, 30}}),
              Emerge.UI.Border.rounded(6.0),
              scrollbar_x(),
              scrollbar_y()
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
            el(
              [Emerge.UI.Font.size(12.0), Emerge.UI.Font.color(:white)],
              text("Align + tuple pad")
            )
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
              above(
                el(
                  [
                    padding(2.0),
                    Emerge.UI.Background.color(:gray),
                    Emerge.UI.Font.size(10.0),
                    Emerge.UI.Font.color(:white)
                  ],
                  text("Above")
                )
              ),
              below(
                el(
                  [
                    padding(2.0),
                    Emerge.UI.Background.color(:gray),
                    Emerge.UI.Font.size(10.0),
                    Emerge.UI.Font.color(:white)
                  ],
                  text("Below")
                )
              ),
              on_left(
                el(
                  [
                    padding(2.0),
                    Emerge.UI.Background.color(:gray),
                    Emerge.UI.Font.size(10.0),
                    Emerge.UI.Font.color(:white)
                  ],
                  text("Left")
                )
              ),
              on_right(
                el(
                  [
                    padding(2.0),
                    Emerge.UI.Background.color(:gray),
                    Emerge.UI.Font.size(10.0),
                    Emerge.UI.Font.color(:white)
                  ],
                  text("Right")
                )
              ),
              in_front(
                el(
                  [
                    padding(2.0),
                    Emerge.UI.Background.color(:gray),
                    Emerge.UI.Font.size(10.0),
                    Emerge.UI.Font.color(:white)
                  ],
                  text("Front")
                )
              ),
              behind_content(
                el(
                  [
                    padding(2.0),
                    Emerge.UI.Background.color(:gray),
                    Emerge.UI.Font.size(10.0),
                    Emerge.UI.Font.color(:white)
                  ],
                  text("Behind")
                )
              )
            ],
            el(
              [
                Emerge.UI.Font.size(16.0),
                Emerge.UI.Font.color(:white),
                Emerge.UI.Font.family("Fira Sans")
              ],
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

  test "nearby elements are preserved through roundtrip" do
    # Create a tree with all nearby element types
    nearby_el =
      el(
        [
          padding(4.0),
          Emerge.UI.Background.color(:gray),
          Emerge.UI.Font.size(10.0),
          Emerge.UI.Font.color(:white)
        ],
        text("Nearby")
      )

    tree =
      el(
        [
          width({:px, 100.0}),
          height({:px, 50.0}),
          above(nearby_el),
          below(nearby_el),
          on_left(nearby_el),
          on_right(nearby_el),
          in_front(nearby_el),
          behind_content(nearby_el)
        ],
        text("Main")
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

  test "EMRG roundtrip preserves paragraph element" do
    tree =
      paragraph(
        [
          width(:fill),
          spacing(4.0),
          Emerge.UI.Font.size(16.0),
          Emerge.UI.Font.color(:white)
        ],
        [
          text("Hello "),
          el([Emerge.UI.Font.bold()], text("world")),
          text(", this wraps automatically.")
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
    assert decoded.type == :paragraph
    assert length(decoded.children) == 3
  end

  test "EMRG roundtrip preserves text_column element" do
    tree =
      text_column(
        [spacing(14.0), center_x()],
        [
          paragraph([spacing(4.0), Emerge.UI.Font.size(14.0)], [
            text("A short intro paragraph for the text column.")
          ]),
          paragraph([spacing(4.0), Emerge.UI.Font.size(14.0)], [
            text("It should keep multiple paragraph children in order."),
            el([Emerge.UI.Font.bold()], text(" Bold spans ")),
            text("also roundtrip correctly.")
          ])
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
    assert decoded.type == :text_column
    assert length(decoded.children) == 2
  end

  test "paragraph inside el produces correct structure" do
    tree =
      el(
        [width({:px, 400}), padding(12.0)],
        paragraph(
          [Emerge.UI.Font.size(16.0), Emerge.UI.Font.color(:white)],
          [text("Hello world")]
        )
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

    assert decoded.type == :el
    assert length(decoded.children) == 1

    [para] = decoded.children
    assert para.type == :paragraph
    assert length(para.children) == 1

    [text_child] = para.children
    assert text_child.type == :text
  end

  test "EMRG roundtrip preserves text_input element" do
    tree =
      Emerge.UI.Input.text("quick brown fox", [
        width(px(260)),
        Emerge.UI.Font.size(14.0),
        on_change({self(), :changed})
      ])

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

    assert assigned.type == :text_input
    assert decoded.type == :text_input
    assert decoded.attrs.content == "quick brown fox"
    assert decoded.attrs.font_size == 14.0
    assert decoded.attrs.on_change == true
  end

  test "EMRG roundtrip preserves new border features" do
    tree =
      el(
        [
          width({:px, 200.0}),
          height({:px, 100.0}),
          Emerge.UI.Border.width_each(1.0, 2.0, 3.0, 4.0),
          Emerge.UI.Border.color(:white),
          Emerge.UI.Border.dashed(),
          Emerge.UI.Border.shadow(offset: {2, 3}, blur: 8, size: 4, color: :red),
          Emerge.UI.Border.glow(:cyan, 3),
          Emerge.UI.Background.color(:black)
        ],
        text("Border test")
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

    # Verify specific attrs
    assert decoded.attrs[:border_width] == {1.0, 2.0, 3.0, 4.0}
    assert decoded.attrs[:border_style] == :dashed
    assert is_list(decoded.attrs[:box_shadow])
    assert length(decoded.attrs[:box_shadow]) == 2
  end

  test "nearby element named colors are preserved" do
    # Test that named colors like :cyan are handled correctly
    tree =
      el(
        [
          width({:px, 100.0}),
          height({:px, 50.0}),
          Emerge.UI.Background.color(:cyan),
          Emerge.UI.Font.color(:cyan)
        ],
        text("Cyan text")
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

    # Verify colors are preserved on the parent element
    assert decoded.attrs[:background] == :cyan
    # Font color is inherited from parent during Rust rendering (not copied to text child)
    assert decoded.attrs[:font_color] == :cyan
  end

  test "EMRG roundtrip preserves image element and background image" do
    tree =
      column([spacing(10.0)], [
        image("img_photo", [width(px(320)), height(px(180)), image_fit(:cover)]),
        el(
          [width(px(200)), height(px(80)), Emerge.UI.Background.image("img_bg", fit: :contain)],
          none()
        )
      ])

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

    assert normalize_tree(decoded) == normalize_tree(assigned)
    [image_node, bg_node] = decoded.children
    assert image_node.type == :image
    assert image_node.attrs.image_src == "img_photo"
    assert image_node.attrs.image_fit == :cover
    assert bg_node.attrs.background == {:image, "img_bg", :contain}
  end

  test "EMRG roundtrip preserves background image repeat fit variants" do
    tree =
      column([spacing(8.0)], [
        el([width(px(80)), height(px(80)), Emerge.UI.Background.tiled("img_bg")], none()),
        el([width(px(80)), height(px(80)), Emerge.UI.Background.tiled_x("img_bg")], none()),
        el([width(px(80)), height(px(80)), Emerge.UI.Background.tiled_y("img_bg")], none()),
        el([width(px(80)), height(px(80)), Emerge.UI.Background.uncropped("img_bg")], none()),
        el([width(px(80)), height(px(80)), Emerge.UI.Background.image("img_bg")], none())
      ])

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

    [tiled, tiled_x, tiled_y, uncropped, image_default] = decoded.children

    assert tiled.attrs.background == {:image, "img_bg", :repeat}
    assert tiled_x.attrs.background == {:image, "img_bg", :repeat_x}
    assert tiled_y.attrs.background == {:image, "img_bg", :repeat_y}
    assert uncropped.attrs.background == {:image, "img_bg", :contain}
    assert image_default.attrs.background == {:image, "img_bg", :cover}
  end
end
