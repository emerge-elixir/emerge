defmodule Emerge.SerializationTest do
  use ExUnit.Case, async: true

  import Emerge.UI

  alias Emerge.Serialization
  alias Emerge.Tree

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
        el(text("a")),
        el(text("b"))
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
          el(text("a")),
          el(text("b"))
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

  defp strip_runtime(%Emerge.Element{} = element) do
    %{
      element
      | attrs: Emerge.Tree.strip_runtime_attrs(element.attrs),
        children: Enum.map(element.children, &strip_runtime/1)
    }
  end
end
