defmodule Emerge.Engine.Serialization do
  @moduledoc """
  Binary serialization for Emerge.Engine.Element trees.
  """

  alias Emerge.Engine.Element
  alias Emerge.Engine.Reconcile
  alias Emerge.Engine.Tree.Nearby

  @version 7

  @type_tag %{
    row: 1,
    wrapped_row: 2,
    column: 3,
    el: 4,
    text: 5,
    none: 6,
    paragraph: 7,
    text_column: 8,
    image: 9,
    text_input: 10,
    video: 11,
    multiline: 12
  }

  @tag_type Map.new(@type_tag, fn {type, tag} -> {tag, type} end)

  @doc """
  Assign ids and encode a tree into a binary payload.
  """
  @spec encode(Element.t()) :: {binary(), Element.t()}
  def encode(tree) do
    {_vdom, tree} = Reconcile.assign_ids(tree)
    {encode_tree(tree), tree}
  end

  @doc """
  Decode a binary payload into a tree.
  """
  @spec decode(binary()) :: Element.t()
  def decode(<<"EMRG", version::unsigned-8, node_count::unsigned-32, rest::binary>>) do
    if version != @version do
      raise ArgumentError, "unsupported serialization version: #{version}"
    end

    {nodes, <<>>} = decode_nodes(rest, node_count, [])

    [root | _] = nodes
    node_map = Map.new(nodes, fn node -> {node.id, node} end)
    build_node(root.id, node_map)
  end

  def decode(_other) do
    raise ArgumentError, "invalid serialization header"
  end

  @doc """
  Encode a tree that already has numeric ids assigned.
  """
  @spec encode_tree(Element.t()) :: binary()
  def encode_tree(%Element{} = tree) do
    nodes = collect_nodes(tree)
    node_count = length(nodes)

    encoded_nodes =
      Enum.map(nodes, fn %{
                           id: id,
                           type: type,
                           attrs: attrs,
                           children: children,
                           nearby: nearby
                         } ->
        type_tag = Map.fetch!(@type_tag, type)
        attr_bin = Emerge.Engine.AttrCodec.encode_attrs(attrs)
        child_ids = Enum.map(children, & &1.id)
        child_count = length(child_ids)
        children_bin = encode_ids(child_ids)
        nearby_count = length(nearby)
        nearby_bin = encode_nearby(nearby)

        <<
          id::unsigned-big-64,
          type_tag::unsigned-8,
          byte_size(attr_bin)::unsigned-32,
          attr_bin::binary,
          child_count::unsigned-16,
          children_bin::binary,
          nearby_count::unsigned-16,
          nearby_bin::binary
        >>
      end)

    header = <<"EMRG", @version::unsigned-8, node_count::unsigned-32>>

    [header | encoded_nodes]
    |> IO.iodata_to_binary()
  end

  defp decode_nodes(rest, 0, acc), do: {Enum.reverse(acc), rest}

  defp decode_nodes(
         <<id::unsigned-big-64, type_tag::unsigned-8, attr_len::unsigned-32, rest::binary>>,
         count,
         acc
       ) do
    <<attr_bin::binary-size(attr_len), rest::binary>> = rest
    attrs = Emerge.Engine.AttrCodec.decode_attrs(attr_bin)
    <<child_count::unsigned-16, rest::binary>> = rest
    {child_ids, rest} = decode_ids(rest, child_count, [])
    <<nearby_count::unsigned-16, rest::binary>> = rest
    {nearby_ids, rest} = decode_nearby(rest, nearby_count, [])

    type = Map.fetch!(@tag_type, type_tag)

    node = %{
      id: id,
      type: type,
      attrs: attrs,
      child_ids: child_ids,
      nearby_ids: nearby_ids
    }

    decode_nodes(rest, count - 1, [node | acc])
  end

  defp build_node(id, node_map) do
    node = Map.fetch!(node_map, id)

    children =
      Enum.map(node.child_ids, fn child_id ->
        build_node(child_id, node_map)
      end)

    nearby =
      Enum.map(node.nearby_ids, fn {slot, nearby_id} ->
        {slot, build_node(nearby_id, node_map)}
      end)

    %Element{
      type: node.type,
      key: nil,
      id: node.id,
      attrs: node.attrs,
      children: children,
      nearby: nearby,
      frame: nil
    }
  end

  defp collect_nodes(%Element{} = element) do
    do_collect_nodes([element], [])
  end

  defp do_collect_nodes([], acc), do: Enum.reverse(acc)

  defp do_collect_nodes([%Element{} = element | rest], acc) do
    nearby_children = Enum.map(element.nearby, fn {_slot, child} -> child end)

    node = %{
      id: element.id,
      type: element.type,
      attrs: element.attrs,
      children: element.children,
      nearby: element.nearby
    }

    do_collect_nodes(element.children ++ nearby_children ++ rest, [node | acc])
  end

  defp encode_ids(ids) do
    ids
    |> Enum.map(fn id ->
      <<id::unsigned-big-64>>
    end)
    |> IO.iodata_to_binary()
  end

  defp decode_ids(rest, 0, acc), do: {Enum.reverse(acc), rest}

  defp decode_ids(<<id::unsigned-big-64, rest::binary>>, count, acc) do
    decode_ids(rest, count - 1, [id | acc])
  end

  defp encode_nearby(nearby) do
    nearby
    |> Enum.map(fn {slot, %Element{id: id}} ->
      <<Nearby.slot_tag(slot)::unsigned-8, id::unsigned-big-64>>
    end)
    |> IO.iodata_to_binary()
  end

  defp decode_nearby(rest, 0, acc), do: {Enum.reverse(acc), rest}

  defp decode_nearby(<<slot_tag::unsigned-8, id::unsigned-big-64, rest::binary>>, count, acc) do
    decode_nearby(rest, count - 1, [{Nearby.slot_from_tag!(slot_tag), id} | acc])
  end
end
