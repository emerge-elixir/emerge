defmodule Emerge.ControlledScrollTest do
  use ExUnit.Case, async: true
  use Emerge.UI

  alias Emerge.Engine.{AttrCodec, EventRegistry, Patch, Reconcile, ScrollTarget, Serialization}

  defp view(value, children \\ [:first, :target]) do
    column(
      [width(px(200)), height(px(100)), scrollbar_y(), scroll_y(value)],
      Enum.map(children, fn key -> el([key(key), height(px(100))], none()) end)
    )
  end

  test "fractions are declarations, survive serialization and participate in diffing" do
    {old, assigned, next} = Reconcile.assign_ids(view(0.0), 1)
    encoded = Serialization.encode_tree(assigned)
    assert Serialization.decode(encoded).attrs.scroll_position_y == 0.0
    {_, patches, _} = Reconcile.reconcile_patches(old, view(0.5), next)
    assert [{:set_attrs, _, %{scroll_position_y: 0.5}}] = patches
    assert Patch.decode(Patch.encode(patches)) == patches
    assert {:scroll_position_x, 1.0} = scroll_x(1.0)

    for value <- [-0.1, 1.1, nil] do
      assert_raise ArgumentError, fn -> scroll_y(value) end
    end
  end

  test "keys resolve against newly assigned children, not the previous tree" do
    {old, _, next} = Reconcile.assign_ids(view(0.0, [:first]), 1)
    {vnode, patches, _} = Reconcile.reconcile_patches(old, view(:target), next)
    target = Enum.find(vnode.children, &(&1.key == :target))
    assert %ScrollTarget{key: :target, declaration: token, id: id} = vnode.attrs.scroll_position_y
    assert id == target.id
    assert token != id
    encoded = Patch.encode(patches)

    assert Enum.any?(Patch.decode(encoded), fn
             {:set_attrs, _,
              %{scroll_position_y: %ScrollTarget{key: nil, declaration: ^token, id: ^id}}} ->
               true

             _ ->
               false
           end)
  end

  test "unchanged keys and style-only updates do not generate new declarations" do
    {old, _, next} = Reconcile.assign_ids(view(:target), 1)
    {new, patches, next} = Reconcile.reconcile_patches(old, view(:target), next)
    assert patches == []
    assert new.attrs.scroll_position_y == old.attrs.scroll_position_y
    changed = %{view(:target) | attrs: Map.put(view(:target).attrs, :font_size, 20)}
    {newer, _, _} = Reconcile.reconcile_patches(new, changed, next)
    assert newer.attrs.scroll_position_y == old.attrs.scroll_position_y
  end

  test "target remount refreshes identity but retains declaration token" do
    {old, _, next} = Reconcile.assign_ids(view(:target), 1)
    [first, _] = view(:target).children

    replacement = %{
      view(:target)
      | children: [first, column([key(:target), height(px(200))], [])]
    }

    {new, _, _} = Reconcile.reconcile_patches(old, replacement, next)

    assert %ScrollTarget{key: :target, declaration: token, id: old_id} =
             old.attrs.scroll_position_y

    assert %ScrollTarget{key: :target, declaration: ^token, id: new_id} =
             new.attrs.scroll_position_y

    refute old_id == new_id
  end

  test "missing keys have an inert numeric target and never serialize arbitrary terms" do
    {_, assigned, _} = Reconcile.assign_ids(view({:missing, make_ref()}), 1)
    assert %ScrollTarget{declaration: token, id: 0} = assigned.attrs.scroll_position_y
    decoded = Serialization.decode(Serialization.encode_tree(assigned))
    assert %ScrollTarget{key: nil, declaration: ^token, id: 0} = decoded.attrs.scroll_position_y
  end

  test "scroll callbacks use normal process and tuple-payload routing" do
    handler = Event.on_scroll({self(), {:solve_event, :scrolled}})
    assert {:on_scroll, {pid, {:solve_event, :scrolled}}} = handler
    attrs = Map.new([handler])
    assert EventRegistry.node_events(attrs) == %{scroll: {pid, {:solve_event, :scrolled}}}
    assert AttrCodec.decode_attrs(AttrCodec.encode_attrs(attrs)) == %{on_scroll: true}
    assert Event.on_scroll(:scrolled) == {:on_scroll, {self(), :scrolled}}
  end

  test "macOS host delivers the same structured fractions and rejects malformed payloads" do
    id = <<42::64>>
    payload = "0.25,1.0"
    frame = <<19, 1, 8::32, id::binary, byte_size(payload)::32, payload::binary>>

    assert {:ok, {^id, :scroll, %{x: 0.25, y: 1.0}}} =
             EmergeSkia.Macos.Protocol.decode_element_event_payload(frame)

    for bad <- ["bad,1.0", "-0.1,0.5", "0.1,1.1", "0.1,0.2,0.3"] do
      assert :error =
               EmergeSkia.Macos.Protocol.decode_element_event_payload(
                 <<19, 1, 8::32, id::binary, byte_size(bad)::32, bad::binary>>
               )
    end

    assert :error =
             EmergeSkia.Macos.Protocol.decode_element_event_payload(
               <<19, 0, 8::32, id::binary, 0::32>>
             )
  end
end
