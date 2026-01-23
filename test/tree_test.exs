defmodule EmergeSkia.TreeTest do
  use ExUnit.Case

  alias EmergeSkia.Native

  # Helper to build EMRG header (version 2)
  defp make_header(node_count) do
    "EMRG" <> <<2, node_count::unsigned-32>>
  end

  # Empty attrs block (attr_count = 0)
  defp empty_attrs, do: <<0::unsigned-16>>

  # Build attrs block with spacing attribute
  defp attrs_with_spacing(spacing) do
    # attr_count=1, tag=4 (spacing), f64 value
    <<1::unsigned-16, 4, spacing::float-64>>
  end

  # Build attrs block with width and height
  defp attrs_with_size(width_px, height_px) do
    # attr_count=2
    # tag=1 (width), variant=2 (px), f64
    # tag=2 (height), variant=2 (px), f64
    <<2::unsigned-16, 1, 2, width_px::float-64, 2, 2, height_px::float-64>>
  end

  describe "tree_new/0" do
    test "creates empty tree" do
      tree = Native.tree_new()
      assert is_reference(tree)
      assert Native.tree_is_empty(tree)
      assert Native.tree_node_count(tree) == 0
    end
  end

  describe "tree_upload/2" do
    test "rejects invalid magic" do
      tree = Native.tree_new()
      result = Native.tree_upload(tree, "XXXX\x02\x00\x00\x00\x00")
      assert {:error, _} = result
    end

    test "accepts empty tree" do
      tree = Native.tree_new()
      data = make_header(0)
      assert {:ok, :ok} = Native.tree_upload(tree, data)
      assert Native.tree_is_empty(tree)
    end

    test "parses single node with empty attrs" do
      tree = Native.tree_new()

      id = :erlang.term_to_binary(:my_node)
      attrs = empty_attrs()

      node_data =
        <<byte_size(id)::unsigned-32>> <>
          id <>
          <<4>> <>
          <<byte_size(attrs)::unsigned-32>> <>
          attrs <>
          <<0::unsigned-16>>

      data = make_header(1) <> node_data
      assert {:ok, :ok} = Native.tree_upload(tree, data)
      assert Native.tree_node_count(tree) == 1
      refute Native.tree_is_empty(tree)
    end

    test "parses node with spacing attribute" do
      tree = Native.tree_new()

      id = :erlang.term_to_binary(:column_node)
      attrs = attrs_with_spacing(10.0)

      node_data =
        <<byte_size(id)::unsigned-32>> <>
          id <>
          <<3>> <>
          <<byte_size(attrs)::unsigned-32>> <>
          attrs <>
          <<0::unsigned-16>>

      data = make_header(1) <> node_data
      assert {:ok, :ok} = Native.tree_upload(tree, data)
      assert Native.tree_node_count(tree) == 1
    end

    test "parses node with width and height" do
      tree = Native.tree_new()

      id = :erlang.term_to_binary(:sized_node)
      attrs = attrs_with_size(100.0, 200.0)

      node_data =
        <<byte_size(id)::unsigned-32>> <>
          id <>
          <<4>> <>
          <<byte_size(attrs)::unsigned-32>> <>
          attrs <>
          <<0::unsigned-16>>

      data = make_header(1) <> node_data
      assert {:ok, :ok} = Native.tree_upload(tree, data)
      assert Native.tree_node_count(tree) == 1
    end

    test "parses tree with children" do
      tree = Native.tree_new()

      parent_id = :erlang.term_to_binary(:parent)
      child1_id = :erlang.term_to_binary(:child1)
      child2_id = :erlang.term_to_binary(:child2)
      attrs = empty_attrs()

      parent_children =
        <<byte_size(child1_id)::unsigned-32>> <> child1_id <>
          <<byte_size(child2_id)::unsigned-32>> <> child2_id

      parent_node =
        <<byte_size(parent_id)::unsigned-32>> <>
          parent_id <>
          <<3>> <>
          <<byte_size(attrs)::unsigned-32>> <>
          attrs <>
          <<2::unsigned-16>> <>
          parent_children

      child1_node =
        <<byte_size(child1_id)::unsigned-32>> <>
          child1_id <>
          <<4>> <>
          <<byte_size(attrs)::unsigned-32>> <>
          attrs <>
          <<0::unsigned-16>>

      child2_node =
        <<byte_size(child2_id)::unsigned-32>> <>
          child2_id <>
          <<5>> <>
          <<byte_size(attrs)::unsigned-32>> <>
          attrs <>
          <<0::unsigned-16>>

      data = make_header(3) <> parent_node <> child1_node <> child2_node
      assert {:ok, :ok} = Native.tree_upload(tree, data)
      assert Native.tree_node_count(tree) == 3
    end
  end

  describe "tree_clear/1" do
    test "clears the tree" do
      tree = Native.tree_new()

      id = :erlang.term_to_binary(1)
      attrs = empty_attrs()

      node_data =
        <<byte_size(id)::unsigned-32>> <>
          id <>
          <<4>> <>
          <<byte_size(attrs)::unsigned-32>> <>
          attrs <>
          <<0::unsigned-16>>

      data = make_header(1) <> node_data
      assert {:ok, :ok} = Native.tree_upload(tree, data)
      assert Native.tree_node_count(tree) == 1

      assert :ok = Native.tree_clear(tree)
      assert Native.tree_is_empty(tree)
      assert Native.tree_node_count(tree) == 0
    end
  end
end
