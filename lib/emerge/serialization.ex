defmodule Emerge.Serialization do
  @moduledoc """
  Binary serialization for Emerge.Element trees.
  """

  alias Emerge.Element
  alias Emerge.Reconcile
  alias Emerge.Tree.Nearby

  @version 4

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
    video: 11
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
      Enum.map(nodes, fn %{id: id, type: type, attrs: attrs, children: children, nearby: nearby} ->
        type_tag = Map.fetch!(@type_tag, type)
        attr_bin = Emerge.AttrCodec.encode_attrs(attrs)
        child_ids = Enum.map(children, & &1.id)
        child_count = length(child_ids)
        children_bin = encode_ids(child_ids)
        {nearby_mask, nearby_ids} = encode_nearby(nearby)
        nearby_bin = encode_ids(nearby_ids)
        id_bin = :erlang.term_to_binary(id)

        <<
          byte_size(id_bin)::unsigned-32,
          id_bin::binary,
          type_tag::unsigned-8,
          byte_size(attr_bin)::unsigned-32,
          attr_bin::binary,
          child_count::unsigned-16,
          children_bin::binary,
          nearby_mask::unsigned-8,
          nearby_bin::binary
        >>
      end)

    header = <<"EMRG", @version::unsigned-8, node_count::unsigned-32>>

    [header | encoded_nodes]
    |> IO.iodata_to_binary()
  end

  defp decode_nodes(rest, 0, acc), do: {Enum.reverse(acc), rest}

  defp decode_nodes(<<id_len::unsigned-32, rest::binary>>, count, acc) do
    <<id_bin::binary-size(id_len), rest::binary>> = rest
    id = :erlang.binary_to_term(id_bin)
    <<type_tag::unsigned-8, attr_len::unsigned-32, rest::binary>> = rest
    <<attr_bin::binary-size(attr_len), rest::binary>> = rest
    attrs = Emerge.AttrCodec.decode_attrs(attr_bin)
    <<child_count::unsigned-16, rest::binary>> = rest
    {child_ids, rest} = decode_ids(rest, child_count, [])
    <<nearby_mask::unsigned-8, rest::binary>> = rest
    {nearby_ids, rest} = decode_nearby(rest, nearby_mask)

    type = Map.fetch!(@tag_type, type_tag)

    node = %{id: id, type: type, attrs: attrs, child_ids: child_ids, nearby_ids: nearby_ids}
    decode_nodes(rest, count - 1, [node | acc])
  end

  defp build_node(id, node_map) do
    node = Map.fetch!(node_map, id)

    children =
      Enum.map(node.child_ids, fn child_id ->
        build_node(child_id, node_map)
      end)

    nearby =
      Enum.reduce(Nearby.nearby_slots(), %{}, fn slot, acc ->
        case Map.get(node.nearby_ids, slot) do
          nil -> acc
          nearby_id -> Map.put(acc, slot, build_node(nearby_id, node_map))
        end
      end)

    %Element{
      type: node.type,
      id: node.id,
      attrs: Nearby.merge_nearby_attrs(node.attrs, nearby),
      children: children,
      frame: nil
    }
  end

  defp collect_nodes(%Element{} = element) do
    {attrs, nearby} = Nearby.split_nearby_attrs(element.attrs)

    [
      %{
        id: element.id,
        type: element.type,
        attrs: attrs,
        children: element.children,
        nearby: nearby
      }
      | Enum.flat_map(element.children, &collect_nodes/1) ++
          Enum.flat_map(Nearby.nearby_children(element), fn {_slot, child} ->
            collect_nodes(child)
          end)
    ]
  end

  defp encode_ids(ids) do
    ids
    |> Enum.map(fn id ->
      bin = :erlang.term_to_binary(id)
      [<<byte_size(bin)::unsigned-32>>, bin]
    end)
    |> IO.iodata_to_binary()
  end

  defp decode_ids(rest, 0, acc), do: {Enum.reverse(acc), rest}

  defp decode_ids(<<len::unsigned-32, rest::binary>>, count, acc) do
    <<id_bin::binary-size(len), rest::binary>> = rest
    id = :erlang.binary_to_term(id_bin)
    decode_ids(rest, count - 1, [id | acc])
  end

  defp encode_nearby(nearby) do
    Nearby.nearby_slots()
    |> Enum.with_index()
    |> Enum.reduce({0, []}, fn {slot, index}, {mask, ids} ->
      case Map.get(nearby, slot) do
        %Element{id: id} -> {Bitwise.bor(mask, Bitwise.bsl(1, index)), ids ++ [id]}
        _ -> {mask, ids}
      end
    end)
  end

  defp decode_nearby(rest, mask) do
    Nearby.nearby_slots()
    |> Enum.with_index()
    |> Enum.reduce({%{}, rest}, fn {slot, index}, {nearby, next_rest} ->
      if Bitwise.band(mask, Bitwise.bsl(1, index)) == 0 do
        {nearby, next_rest}
      else
        {ids, remaining} = decode_ids(next_rest, 1, [])
        {Map.put(nearby, slot, hd(ids)), remaining}
      end
    end)
  end
end
