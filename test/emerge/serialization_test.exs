defmodule Emerge.Engine.SerializationTest do
  use ExUnit.Case, async: true

  use Emerge.UI

  alias Emerge.Engine.Serialization
  alias Emerge.Engine.Tree

  test "assign_ids assigns stable integer ids" do
    layout =
      column([key(10)], [
        el([key(12)], text("a")),
        el([key(13)], text("b"))
      ])

    {tree, _state} = Tree.assign_ids(layout)

    assert is_integer(tree.id)
    assert is_integer(Enum.at(tree.children, 0).id)
    assert is_integer(Enum.at(tree.children, 1).id)
  end

  test "assign_ids is deterministic for the same tree" do
    layout =
      row([key(:root)], [
        el([key({:card, 1})], text("a")),
        el([key({:card, 2})], text("b"))
      ])

    {tree, _state} = Tree.assign_ids(layout)
    {tree2, _state2} = Tree.assign_ids(layout)

    assert tree.id == tree2.id
    assert Enum.at(tree.children, 0).id == Enum.at(tree2.children, 0).id
    assert Enum.at(tree.children, 1).id == Enum.at(tree2.children, 1).id
  end

  test "encode assigns ids and returns a binary" do
    layout =
      row([spacing(10)], [
        el([], text("a")),
        el([], text("b"))
      ])

    {binary, tree} = Serialization.encode(layout)

    assert is_binary(binary)
    assert is_integer(tree.id)
    assert is_integer(Enum.at(tree.children, 0).id)
    assert is_integer(Enum.at(tree.children, 1).id)
  end

  test "decode returns the same tree as encode (sans runtime attrs)" do
    layout =
      column([spacing(8)], [
        el([padding(4)], text("hello")),
        row([spacing(2)], [
          el([], text("a")),
          el([], text("b"))
        ])
      ])

    {binary, tree} = Serialization.encode(layout)
    decoded = Serialization.decode(binary)

    assert strip_runtime(decoded) == strip_runtime(tree)
  end

  test "paragraph roundtrip preserves type and children" do
    layout =
      paragraph([spacing(6)], [
        text("Hello "),
        el([Emerge.UI.Font.bold()], text("world")),
        text(", this wraps.")
      ])

    {binary, tree} = Serialization.encode(layout)
    decoded = Serialization.decode(binary)

    assert strip_runtime(decoded) == strip_runtime(tree)
    assert decoded.type == :paragraph
    assert length(decoded.children) == 3
  end

  test "text_column roundtrip preserves type, defaults, and children" do
    layout =
      text_column([spacing(10)], [
        paragraph([spacing(4)], [text("First paragraph")]),
        paragraph([spacing(4)], [text("Second paragraph")])
      ])

    {binary, tree} = Serialization.encode(layout)
    decoded = Serialization.decode(binary)

    assert strip_runtime(decoded) == strip_runtime(tree)
    assert decoded.type == :text_column
    assert decoded.attrs.width == :fill
    assert decoded.attrs.height == :content
    assert length(decoded.children) == 2
  end

  test "image roundtrip preserves image attrs" do
    layout = image([width(px(300)), height(px(120)), image_fit(:cover)], "img_banner")

    {binary, tree} = Serialization.encode(layout)
    decoded = Serialization.decode(binary)

    assert strip_runtime(decoded) == strip_runtime(tree)
    assert decoded.type == :image
    assert decoded.attrs.image_src == "img_banner"
    assert decoded.attrs.image_fit == :cover
  end

  test "text_input roundtrip preserves content and handlers" do
    layout =
      Emerge.UI.Input.text(
        [
          width(px(280)),
          Event.on_change({self(), :changed}),
          Event.on_focus({self(), :focused}),
          Event.on_blur({self(), :blurred}),
          Interactive.focused([
            Transform.alpha(0.9),
            Transform.move_x(2),
            Emerge.UI.Border.glow(:cyan, 3)
          ]),
          Interactive.mouse_down([
            Transform.move_y(-1),
            Transform.scale(0.98),
            Emerge.UI.Border.inner_shadow(offset: {0, 1}, blur: 6, size: 1, color: :black)
          ])
        ],
        "hello"
      )

    {binary, tree} = Serialization.encode(layout)
    decoded = Serialization.decode(binary)

    assert tree.type == :text_input
    assert decoded.type == :text_input
    assert decoded.attrs.content == "hello"
    assert decoded.attrs.on_change == true
    assert decoded.attrs.on_focus == true
    assert decoded.attrs.on_blur == true

    assert decoded.attrs.focused == %{
             alpha: 0.9,
             move_x: 2.0,
             box_shadow: [
               %{offset_x: 0.0, offset_y: 0.0, blur: 6.0, size: 3.0, color: :cyan, inset: false}
             ]
           }

    assert decoded.attrs.mouse_down == %{
             move_y: -1.0,
             scale: 0.98,
             box_shadow: [
               %{offset_x: 0.0, offset_y: 1.0, blur: 6.0, size: 1.0, color: :black, inset: true}
             ]
           }
  end

  test "input button roundtrip preserves press and focus handlers" do
    layout =
      Emerge.UI.Input.button(
        [
          Event.on_press({self(), :pressed}),
          Event.on_focus({self(), :focused}),
          Event.on_blur({self(), :blurred}),
          Interactive.focused([Transform.alpha(0.9), Emerge.UI.Border.glow(:cyan, 2)]),
          Interactive.mouse_down([
            Transform.move_y(-1),
            Emerge.UI.Border.inner_shadow(offset: {0, 1}, blur: 5, size: 1, color: :black)
          ])
        ],
        Emerge.UI.text("Save")
      )

    {binary, tree} = Serialization.encode(layout)
    decoded = Serialization.decode(binary)

    assert tree.type == :el
    assert decoded.type == :el
    assert decoded.attrs.on_press == true
    assert decoded.attrs.on_focus == true
    assert decoded.attrs.on_blur == true

    assert decoded.attrs.focused == %{
             alpha: 0.9,
             box_shadow: [
               %{offset_x: 0.0, offset_y: 0.0, blur: 4.0, size: 2.0, color: :cyan, inset: false}
             ]
           }

    assert decoded.attrs.mouse_down == %{
             move_y: -1.0,
             box_shadow: [
               %{offset_x: 0.0, offset_y: 1.0, blur: 5.0, size: 1.0, color: :black, inset: true}
             ]
           }

    assert length(decoded.children) == 1
    assert hd(decoded.children).type == :text
    assert hd(decoded.children).attrs.content == "Save"
  end

  test "direct state style maps are normalized before serialization" do
    shadow = %{offset_x: 0, offset_y: 1, blur: 6, size: 2, color: :black, inset: true}

    layout =
      el(
        [
          {:mouse_over, %{alpha: 0.75, box_shadow: shadow}}
        ],
        text("Hello")
      )

    {binary, tree} = Serialization.encode(layout)
    decoded = Serialization.decode(binary)

    assert tree.attrs.mouse_over == %{alpha: 0.75, box_shadow: [shadow]}

    assert decoded.attrs.mouse_over == %{
             alpha: 0.75,
             box_shadow: [
               %{offset_x: 0.0, offset_y: 1.0, blur: 6.0, size: 2.0, color: :black, inset: true}
             ]
           }
  end

  defp strip_runtime(%Emerge.Engine.Element{} = element) do
    %{
      element
      | attrs: Emerge.Engine.Tree.strip_runtime_attrs(element.attrs),
        children: Enum.map(element.children, &strip_runtime/1)
    }
  end
end
