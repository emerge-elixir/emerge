defmodule EmergeSkia.TreeTest do
  use ExUnit.Case

  alias Emerge.Engine.NodeId
  alias EmergeSkia.Native

  # Helper to build EMRG header (version 7)
  defp make_header(node_count) do
    "EMRG" <> <<7, node_count::unsigned-32>>
  end

  defp encode_node(id, type_tag, attrs, child_ids \\ [], nearby \\ []) do
    <<
      id::unsigned-big-64,
      type_tag::unsigned-8,
      byte_size(attrs)::unsigned-32,
      attrs::binary,
      length(child_ids)::unsigned-16,
      encode_ids(child_ids)::binary,
      length(nearby)::unsigned-16,
      encode_nearby(nearby)::binary
    >>
  end

  defp encode_ids(ids) do
    ids
    |> Enum.map(fn id -> <<id::unsigned-big-64>> end)
    |> IO.iodata_to_binary()
  end

  defp encode_nearby(mounts) do
    mounts
    |> Enum.map(fn {slot_tag, id} -> <<slot_tag::unsigned-8, id::unsigned-big-64>> end)
    |> IO.iodata_to_binary()
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
      assert {:ok, true} = Native.tree_upload(tree, data)
      assert Native.tree_is_empty(tree)
    end

    test "parses single node with empty attrs" do
      tree = Native.tree_new()

      id = 1
      attrs = empty_attrs()

      node_data = encode_node(id, 4, attrs)

      data = make_header(1) <> node_data
      assert {:ok, true} = Native.tree_upload(tree, data)
      assert Native.tree_node_count(tree) == 1
      refute Native.tree_is_empty(tree)
    end

    test "parses node with spacing attribute" do
      tree = Native.tree_new()

      id = 2
      attrs = attrs_with_spacing(10.0)

      node_data = encode_node(id, 3, attrs)

      data = make_header(1) <> node_data
      assert {:ok, true} = Native.tree_upload(tree, data)
      assert Native.tree_node_count(tree) == 1
    end

    test "parses node with width and height" do
      tree = Native.tree_new()

      id = 3
      attrs = attrs_with_size(100.0, 200.0)

      node_data = encode_node(id, 4, attrs)

      data = make_header(1) <> node_data
      assert {:ok, true} = Native.tree_upload(tree, data)
      assert Native.tree_node_count(tree) == 1
    end

    test "parses tree with children" do
      tree = Native.tree_new()

      parent_id = 10
      child1_id = 11
      child2_id = 12
      attrs = empty_attrs()

      parent_node = encode_node(parent_id, 3, attrs, [child1_id, child2_id])
      child1_node = encode_node(child1_id, 4, attrs)
      child2_node = encode_node(child2_id, 5, attrs)

      data = make_header(3) <> parent_node <> child1_node <> child2_node
      assert {:ok, true} = Native.tree_upload(tree, data)
      assert Native.tree_node_count(tree) == 3
    end
  end

  describe "tree_clear/1" do
    test "clears the tree" do
      tree = Native.tree_new()

      id = 1
      attrs = empty_attrs()

      node_data = encode_node(id, 4, attrs)

      data = make_header(1) <> node_data
      assert {:ok, true} = Native.tree_upload(tree, data)
      assert Native.tree_node_count(tree) == 1

      assert :ok = Native.tree_clear(tree)
      assert Native.tree_is_empty(tree)
      assert Native.tree_node_count(tree) == 0
    end
  end

  describe "tree_layout/4" do
    test "layouts single element with fixed size" do
      tree = Native.tree_new()

      id = 20
      attrs = attrs_with_size(100.0, 50.0)

      node_data = encode_node(id, 4, attrs)

      data = make_header(1) <> node_data
      assert {:ok, true} = Native.tree_upload(tree, data)

      {:ok, frames} = Native.tree_layout(tree, 800.0, 600.0, 1.0)
      assert length(frames) == 1

      [{frame_id, x, y, w, h}] = frames
      assert NodeId.decode(frame_id) == id
      assert x == 0.0
      assert y == 0.0
      assert w == 100.0
      assert h == 50.0
    end

    test "layouts row with two children" do
      tree = Native.tree_new()

      row_id = 30
      child1_id = 31
      child2_id = 32

      row_attrs = attrs_with_spacing(10.0)
      child_attrs = attrs_with_size(50.0, 30.0)

      row_node = encode_node(row_id, 1, row_attrs, [child1_id, child2_id])
      child1_node = encode_node(child1_id, 4, child_attrs)
      child2_node = encode_node(child2_id, 4, child_attrs)

      data = make_header(3) <> row_node <> child1_node <> child2_node
      assert {:ok, true} = Native.tree_upload(tree, data)

      {:ok, frames} = Native.tree_layout(tree, 800.0, 600.0, 1.0)
      assert length(frames) == 3

      frames_map = Map.new(frames, fn {id, x, y, w, h} -> {NodeId.decode(id), {x, y, w, h}} end)

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

      col_id = 40
      child1_id = 41
      child2_id = 42

      # Column with fixed height
      col_attrs = <<2::unsigned-16, 2, 2, 100.0::float-64, 1, 2, 50.0::float-64>>

      # Children with fill height (tag=2, variant=0 for fill)
      child_attrs = <<2::unsigned-16, 1, 2, 50.0::float-64, 2, 0>>

      col_node = encode_node(col_id, 3, col_attrs, [child1_id, child2_id])
      child1_node = encode_node(child1_id, 4, child_attrs)
      child2_node = encode_node(child2_id, 4, child_attrs)

      data = make_header(3) <> col_node <> child1_node <> child2_node
      assert {:ok, true} = Native.tree_upload(tree, data)

      {:ok, frames} = Native.tree_layout(tree, 800.0, 600.0, 1.0)
      assert length(frames) == 3

      frames_map = Map.new(frames, fn {id, x, y, w, h} -> {NodeId.decode(id), {x, y, w, h}} end)

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

      id = 50
      # Element with width=100px, height=50px, padding=10px
      attrs = attrs_with_size_and_padding(100.0, 50.0, 10.0)

      node_data = encode_node(id, 4, attrs)

      data = make_header(1) <> node_data
      assert {:ok, true} = Native.tree_upload(tree, data)

      # With scale=2.0, width should be 200, height should be 100
      {:ok, frames} = Native.tree_layout(tree, 800.0, 600.0, 2.0)
      assert length(frames) == 1

      [{_frame_id, x, y, w, h}] = frames
      assert x == 0.0
      assert y == 0.0
      assert w == 200.0
      assert h == 100.0
    end

    test "reports gated stats from retained layout" do
      tree = Native.tree_new()

      root_id = 90
      child1_id = 91
      child2_id = 92
      child_attrs = attrs_with_size(50.0, 30.0)

      root_node = encode_node(root_id, 3, empty_attrs(), [child1_id, child2_id])
      child1_node = encode_node(child1_id, 4, child_attrs)
      child2_node = encode_node(child2_id, 4, child_attrs)

      data = make_header(3) <> root_node <> child1_node <> child2_node
      assert {:ok, :ok} = Native.tree_upload(tree, data)

      {:ok, disabled_stats} = Native.stats(tree, :peek)
      refute disabled_stats.enabled

      {:ok, _frames} = Native.tree_layout(tree, 800.0, 600.0, 1.0)
      {:ok, disabled_after_layout} = Native.stats(tree, :peek)
      assert disabled_after_layout.counters.layout_cache.resolve_stores == 0

      {:ok, enabled_stats} = Native.stats(tree, {:configure, %{enabled: true}})
      assert enabled_stats.enabled

      {:ok, _frames} = Native.tree_layout(tree, 800.0, 600.0, 1.0)
      {:ok, warm_snapshot} = Native.stats(tree, :peek)
      warm_stats = warm_snapshot.counters.layout_cache
      assert warm_stats.subtree_measure_hits > 0
      assert warm_stats.resolve_hits > 0
      assert warm_snapshot.timings.layout.count == 1

      {:ok, peek_again} = Native.stats(tree, :peek)
      assert peek_again.timings.layout.count == 1

      {:ok, _frames} = Native.tree_layout(tree, 800.0, 600.0, 1.0)
      {:ok, taken_snapshot} = Native.stats(tree, :take)
      taken_stats = taken_snapshot.counters.layout_cache

      assert taken_stats.subtree_measure_hits > 0
      assert taken_stats.resolve_hits > 0
      assert taken_stats.resolve_misses == 0
      assert Map.has_key?(taken_stats, :intrinsic_measure_ineligible_bypasses)
      assert taken_snapshot.window.reset_on_read

      {:ok, reset_snapshot} = Native.stats(tree, :peek)
      assert reset_snapshot.timings.layout.count == 0
      assert reset_snapshot.counters.layout_cache.resolve_hits == 0
    end
  end

  # Helper to create attrs with width, height, and uniform padding
  defp attrs_with_size_and_padding(width, height, padding) do
    # width (tag 1, variant 2=px), height (tag 2, variant 2=px), padding (tag 3, variant 0=uniform)
    <<3::unsigned-16, 1, 2, width::float-64, 2, 2, height::float-64, 3, 0, padding::float-64>>
  end

  describe "wrapped_row layout" do
    test "wrapped_row height expands when items wrap" do
      tree = Native.tree_new()

      row_id = 60
      c1_id = 61
      c2_id = 62
      c3_id = 63

      # Row with 100px width and 10px spacing
      # attr_count=2, width(tag=1, px=100), spacing(tag=4, 10.0)
      row_attrs = <<2::unsigned-16, 1, 2, 100.0::float-64, 4, 10.0::float-64>>

      # Children 50px wide, 30px tall
      child_attrs = attrs_with_size(50.0, 30.0)

      row_node = encode_node(row_id, 2, row_attrs, [c1_id, c2_id, c3_id])
      c1_node = encode_node(c1_id, 4, child_attrs)
      c2_node = encode_node(c2_id, 4, child_attrs)
      c3_node = encode_node(c3_id, 4, child_attrs)

      data = make_header(4) <> row_node <> c1_node <> c2_node <> c3_node
      assert {:ok, true} = Native.tree_upload(tree, data)

      {:ok, frames} = Native.tree_layout(tree, 800.0, 600.0, 1.0)
      frames_map = Map.new(frames, fn {id, x, y, w, h} -> {NodeId.decode(id), {x, y, w, h}} end)

      # With 100px width and children 50px wide:
      # Each child wraps to its own line (50 + 10 spacing + 50 = 110 > 100)
      # Total height = 3 * 30 + 2 * 10 = 110px
      {_row_x, _row_y, _row_w, row_h} = frames_map[row_id]
      assert row_h == 110.0

      # Check child y positions
      {_c1_x, c1_y, _c1_w, _c1_h} = frames_map[c1_id]
      {_c2_x, c2_y, _c2_w, _c2_h} = frames_map[c2_id]
      {_c3_x, c3_y, _c3_w, _c3_h} = frames_map[c3_id]

      assert c1_y == 0.0
      # 30 + 10 spacing
      assert c2_y == 40.0
      # 30 + 10 + 30 + 10 spacing
      assert c3_y == 80.0
    end

    test "wrapped_row with multiple items per line" do
      tree = Native.tree_new()

      row_id = 70
      c1_id = 71
      c2_id = 72
      c3_id = 73
      c4_id = 74

      # Row with 120px width and 10px spacing
      # Two children fit per line: 50 + 10 + 50 = 110 < 120
      row_attrs = <<2::unsigned-16, 1, 2, 120.0::float-64, 4, 10.0::float-64>>

      # Children 50px wide, 30px tall
      child_attrs = attrs_with_size(50.0, 30.0)

      row_node = encode_node(row_id, 2, row_attrs, [c1_id, c2_id, c3_id, c4_id])
      c1_node = encode_node(c1_id, 4, child_attrs)
      c2_node = encode_node(c2_id, 4, child_attrs)
      c3_node = encode_node(c3_id, 4, child_attrs)
      c4_node = encode_node(c4_id, 4, child_attrs)

      data = make_header(5) <> row_node <> c1_node <> c2_node <> c3_node <> c4_node
      assert {:ok, true} = Native.tree_upload(tree, data)

      {:ok, frames} = Native.tree_layout(tree, 800.0, 600.0, 1.0)
      frames_map = Map.new(frames, fn {id, x, y, w, h} -> {NodeId.decode(id), {x, y, w, h}} end)

      # With 120px width: 2 items per line
      # Total height = 2 * 30 + 1 * 10 = 70px
      {_row_x, _row_y, _row_w, row_h} = frames_map[row_id]
      assert row_h == 70.0

      # Check positions: line 1 at y=0, line 2 at y=40
      {c1_x, c1_y, _c1_w, _c1_h} = frames_map[c1_id]
      {c2_x, c2_y, _c2_w, _c2_h} = frames_map[c2_id]
      {c3_x, c3_y, _c3_w, _c3_h} = frames_map[c3_id]
      {c4_x, c4_y, _c4_w, _c4_h} = frames_map[c4_id]

      assert c1_x == 0.0 and c1_y == 0.0
      # 50 + 10 spacing
      assert c2_x == 60.0 and c2_y == 0.0
      # new line
      assert c3_x == 0.0 and c3_y == 40.0
      assert c4_x == 60.0 and c4_y == 40.0
    end

    test "column with wrapped_row pushes sibling down" do
      tree = Native.tree_new()

      col_id = 80
      row_id = 81
      c1_id = 82
      c2_id = 83
      c3_id = 84
      below_id = 85

      # Column with 100px width and 10px spacing
      col_attrs = <<2::unsigned-16, 1, 2, 100.0::float-64, 4, 10.0::float-64>>

      # Wrapped row with fill width and 10px spacing
      # attr_count=2, width(tag=1, fill=0), spacing(tag=4, 10.0)
      row_attrs = <<2::unsigned-16, 1, 0, 4, 10.0::float-64>>

      # Children 50px wide, 30px tall (each will wrap to its own line in 100px container)
      child_attrs = attrs_with_size(50.0, 30.0)

      # Below element: 40px tall, fill width
      below_attrs = <<2::unsigned-16, 1, 0, 2, 2, 40.0::float-64>>

      col_node = encode_node(col_id, 3, col_attrs, [row_id, below_id])
      row_node = encode_node(row_id, 2, row_attrs, [c1_id, c2_id, c3_id])
      c1_node = encode_node(c1_id, 4, child_attrs)
      c2_node = encode_node(c2_id, 4, child_attrs)
      c3_node = encode_node(c3_id, 4, child_attrs)
      below_node = encode_node(below_id, 4, below_attrs)

      data = make_header(6) <> col_node <> row_node <> c1_node <> c2_node <> c3_node <> below_node
      assert {:ok, true} = Native.tree_upload(tree, data)

      {:ok, frames} = Native.tree_layout(tree, 800.0, 600.0, 1.0)
      frames_map = Map.new(frames, fn {id, x, y, w, h} -> {NodeId.decode(id), {x, y, w, h}} end)

      # Wrapped row: 3 lines * 30px + 2 * 10px spacing = 110px
      {_row_x, _row_y, _row_w, row_h} = frames_map[row_id]
      assert row_h == 110.0

      # Below element should be pushed down: y = 110 + 10 spacing = 120
      {_below_x, below_y, _below_w, _below_h} = frames_map[below_id]
      assert below_y == 120.0

      # Column height: 110 + 10 + 40 = 160
      {_col_x, _col_y, _col_w, col_h} = frames_map[col_id]
      assert col_h == 160.0
    end
  end
end
