defmodule Emerge.Engine.NodeId do
  @moduledoc false

  @spec encode(pos_integer()) :: binary()
  def encode(id) when is_integer(id) and id > 0 do
    <<id::unsigned-big-64>>
  end

  @spec decode(binary()) :: non_neg_integer()
  def decode(<<id::unsigned-big-64>>), do: id

  @spec encode_parent(pos_integer() | nil) :: binary()
  def encode_parent(nil), do: <<0::unsigned-big-64>>
  def encode_parent(id) when is_integer(id) and id > 0, do: encode(id)

  @spec decode_parent(binary()) :: non_neg_integer() | nil
  def decode_parent(<<0::unsigned-big-64>>), do: nil
  def decode_parent(binary), do: decode(binary)
end
