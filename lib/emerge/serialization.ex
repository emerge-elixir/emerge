defmodule Emerge.Serialization do
  @moduledoc """
  Binary serialization for Emerge.Element trees.
  """

  alias Emerge.Element
  alias Emerge.Reconcile

  @version 2

  @type_tag %{
    row: 1,
    wrapped_row: 2,
    column: 3,
    el: 4,
    text: 5,
    none: 6,
    paragraph: 7
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
    {nodes, <<>>} =
      case version do
        1 -> decode_nodes_v1(rest, node_count, [])
        2 -> decode_nodes(rest, node_count, [])
        _ -> raise ArgumentError, "unsupported serialization version: #{version}"
      end

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
      Enum.map(nodes, fn %{id: id, type: type, attrs: attrs, children: children} ->
        type_tag = Map.fetch!(@type_tag, type)
        attr_bin = Emerge.AttrCodec.encode_attrs(attrs)
        child_ids = Enum.map(children, & &1.id)
        child_count = length(child_ids)
        children_bin =
          child_ids
          |> Enum.map(fn child_id ->
            bin = :erlang.term_to_binary(child_id)
            [<<byte_size(bin)::unsigned-32>>, bin]
          end)
          |> IO.iodata_to_binary()

        id_bin = :erlang.term_to_binary(id)

        <<
          byte_size(id_bin)::unsigned-32,
          id_bin::binary,
          type_tag::unsigned-8,
          byte_size(attr_bin)::unsigned-32,
          attr_bin::binary,
          child_count::unsigned-16,
          children_bin::binary
        >>
      end)

    header = <<"EMRG", @version::unsigned-8, node_count::unsigned-32>>

    [header | encoded_nodes]
    |> IO.iodata_to_binary()
  end

  defp decode_nodes(rest, 0, acc), do: {Enum.reverse(acc), rest}

  defp decode_nodes(
         <<id_len::unsigned-32, rest::binary>>,
         count,
         acc
       ) do
    <<id_bin::binary-size(id_len), rest::binary>> = rest
    id = :erlang.binary_to_term(id_bin)
    <<type_tag::unsigned-8, attr_len::unsigned-32, rest::binary>> = rest
    <<attr_bin::binary-size(attr_len), rest::binary>> = rest
    attrs = Emerge.AttrCodec.decode_attrs(attr_bin)
    <<child_count::unsigned-16, rest::binary>> = rest
    {child_ids, rest} = decode_child_ids(rest, child_count, [])

    type = Map.fetch!(@tag_type, type_tag)

    node = %{id: id, type: type, attrs: attrs, child_ids: child_ids}
    decode_nodes(rest, count - 1, [node | acc])
  end

  defp decode_nodes_v1(rest, 0, acc), do: {Enum.reverse(acc), rest}

  defp decode_nodes_v1(
         <<id_len::unsigned-32, rest::binary>>,
         count,
         acc
       ) do
    <<id_bin::binary-size(id_len), rest::binary>> = rest
    id = :erlang.binary_to_term(id_bin)
    <<type_tag::unsigned-8, attr_len::unsigned-32, rest::binary>> = rest
    <<attr_bin::binary-size(attr_len), rest::binary>> = rest
    attrs = :erlang.binary_to_term(attr_bin)
    <<child_count::unsigned-16, rest::binary>> = rest
    {child_ids, rest} = decode_child_ids(rest, child_count, [])

    type = Map.fetch!(@tag_type, type_tag)

    node = %{id: id, type: type, attrs: attrs, child_ids: child_ids}
    decode_nodes_v1(rest, count - 1, [node | acc])
  end

  defp build_node(id, node_map) do
    node = Map.fetch!(node_map, id)

    children =
      Enum.map(node.child_ids, fn child_id ->
        build_node(child_id, node_map)
      end)

    %Element{
      type: node.type,
      id: node.id,
      attrs: node.attrs,
      children: children,
      frame: nil
    }
  end

  defp collect_nodes(%Element{} = element) do
    [%{id: element.id, type: element.type, attrs: element.attrs, children: element.children} |
       Enum.flat_map(element.children, &collect_nodes/1)]
  end

  defp decode_child_ids(rest, 0, acc), do: {Enum.reverse(acc), rest}

  defp decode_child_ids(<<len::unsigned-32, rest::binary>>, count, acc) do
    <<id_bin::binary-size(len), rest::binary>> = rest
    id = :erlang.binary_to_term(id_bin)
    decode_child_ids(rest, count - 1, [id | acc])
  end
end
