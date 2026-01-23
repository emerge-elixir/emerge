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

  describe "tree_layout/4" do
    test "layouts single element with fixed size" do
      tree = Native.tree_new()

      id = :erlang.term_to_binary(:root)
      attrs = attrs_with_size(100.0, 50.0)

      node_data =
        <<byte_size(id)::unsigned-32>> <>
          id <>
          <<4>> <>
          <<byte_size(attrs)::unsigned-32>> <>
          attrs <>
          <<0::unsigned-16>>

      data = make_header(1) <> node_data
      assert {:ok, :ok} = Native.tree_upload(tree, data)

      {:ok, frames} = Native.tree_layout(tree, 800.0, 600.0, 1.0)
      assert length(frames) == 1

      [{frame_id, x, y, w, h}] = frames
      assert frame_id == id
      assert x == 0.0
      assert y == 0.0
      assert w == 100.0
      assert h == 50.0
    end

    test "layouts row with two children" do
      tree = Native.tree_new()

      row_id = :erlang.term_to_binary(:row)
      child1_id = :erlang.term_to_binary(:c1)
      child2_id = :erlang.term_to_binary(:c2)

      row_attrs = attrs_with_spacing(10.0)
      child_attrs = attrs_with_size(50.0, 30.0)

      row_children =
        <<byte_size(child1_id)::unsigned-32>> <> child1_id <>
          <<byte_size(child2_id)::unsigned-32>> <> child2_id

      row_node =
        <<byte_size(row_id)::unsigned-32>> <>
          row_id <>
          <<1>> <>
          <<byte_size(row_attrs)::unsigned-32>> <>
          row_attrs <>
          <<2::unsigned-16>> <>
          row_children

      child1_node =
        <<byte_size(child1_id)::unsigned-32>> <>
          child1_id <>
          <<4>> <>
          <<byte_size(child_attrs)::unsigned-32>> <>
          child_attrs <>
          <<0::unsigned-16>>

      child2_node =
        <<byte_size(child2_id)::unsigned-32>> <>
          child2_id <>
          <<4>> <>
          <<byte_size(child_attrs)::unsigned-32>> <>
          child_attrs <>
          <<0::unsigned-16>>

      data = make_header(3) <> row_node <> child1_node <> child2_node
      assert {:ok, :ok} = Native.tree_upload(tree, data)

      {:ok, frames} = Native.tree_layout(tree, 800.0, 600.0, 1.0)
      assert length(frames) == 3

      frames_map = Map.new(frames, fn {id, x, y, w, h} -> {id, {x, y, w, h}} end)

      # Child 1 should be at x=0
      {c1_x, _c1_y, c1_w, _c1_h} = frames_map[child1_id]
      assert c1_x == 0.0
      assert c1_w == 50.0

      # Child 2 should be at x=60 (50 + 10 spacing)
      {c2_x, _c2_y, c2_w, _c2_h} = frames_map[child2_id]
      assert c2_x == 60.0
      assert c2_w == 50.0
    end

    test "layouts column with fill children" do
      tree = Native.tree_new()

      col_id = :erlang.term_to_binary(:col)
      child1_id = :erlang.term_to_binary(:c1)
      child2_id = :erlang.term_to_binary(:c2)

      # Column with fixed height
      col_attrs = <<2::unsigned-16, 2, 2, 100.0::float-64, 1, 2, 50.0::float-64>>

      # Children with fill height (tag=2, variant=0 for fill)
      child_attrs = <<2::unsigned-16, 1, 2, 50.0::float-64, 2, 0>>

      col_children =
        <<byte_size(child1_id)::unsigned-32>> <> child1_id <>
          <<byte_size(child2_id)::unsigned-32>> <> child2_id

      col_node =
        <<byte_size(col_id)::unsigned-32>> <>
          col_id <>
          <<3>> <>
          <<byte_size(col_attrs)::unsigned-32>> <>
          col_attrs <>
          <<2::unsigned-16>> <>
          col_children

      child1_node =
        <<byte_size(child1_id)::unsigned-32>> <>
          child1_id <>
          <<4>> <>
          <<byte_size(child_attrs)::unsigned-32>> <>
          child_attrs <>
          <<0::unsigned-16>>

      child2_node =
        <<byte_size(child2_id)::unsigned-32>> <>
          child2_id <>
          <<4>> <>
          <<byte_size(child_attrs)::unsigned-32>> <>
          child_attrs <>
          <<0::unsigned-16>>

      data = make_header(3) <> col_node <> child1_node <> child2_node
      assert {:ok, :ok} = Native.tree_upload(tree, data)

      {:ok, frames} = Native.tree_layout(tree, 800.0, 600.0, 1.0)
      assert length(frames) == 3

      frames_map = Map.new(frames, fn {id, x, y, w, h} -> {id, {x, y, w, h}} end)

      # Both children should split the 100px height equally
      {_c1_x, c1_y, _c1_w, c1_h} = frames_map[child1_id]
      {_c2_x, c2_y, _c2_w, c2_h} = frames_map[child2_id]

      assert c1_h == 50.0
      assert c2_h == 50.0
      assert c1_y == 0.0
      assert c2_y == 50.0
    end

    test "applies scale factor to pixel values" do
      tree = Native.tree_new()

      id = :erlang.term_to_binary(:root)
      # Element with width=100px, height=50px, padding=10px
      attrs = attrs_with_size_and_padding(100.0, 50.0, 10.0)

      node_data =
        <<byte_size(id)::unsigned-32>> <>
          id <>
          <<4>> <>
          <<byte_size(attrs)::unsigned-32>> <>
          attrs <>
          <<0::unsigned-16>>

      data = make_header(1) <> node_data
      assert {:ok, :ok} = Native.tree_upload(tree, data)

      # With scale=2.0, width should be 200, height should be 100
      {:ok, frames} = Native.tree_layout(tree, 800.0, 600.0, 2.0)
      assert length(frames) == 1

      [{_frame_id, x, y, w, h}] = frames
      assert x == 0.0
      assert y == 0.0
      assert w == 200.0
      assert h == 100.0
    end
  end

  # Helper to create attrs with width, height, and uniform padding
  defp attrs_with_size_and_padding(width, height, padding) do
    # width (tag 1, variant 2=px), height (tag 2, variant 2=px), padding (tag 3, variant 0=uniform)
    <<3::unsigned-16,
      1, 2, width::float-64,
      2, 2, height::float-64,
      3, 0, padding::float-64>>
  end
end
