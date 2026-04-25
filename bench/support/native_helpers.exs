defmodule Emerge.Bench.NativeHelpers do
  @moduledoc false

  alias EmergeSkia.Native

  def upload_tree!(full_bin) do
    tree = Native.tree_new()
    ok!(Native.tree_upload(tree, full_bin))
    tree
  end

  def ok!(:ok), do: :ok
  def ok!({:ok, :ok}), do: :ok
  def ok!({:error, reason}), do: raise("native benchmark failed: #{reason}")
  def ok!(other), do: raise("unexpected native ok result: #{inspect(other)}")

  def unwrap!({:ok, value}), do: value
  def unwrap!({:error, reason}), do: raise("native benchmark failed: #{reason}")
  def unwrap!(value) when is_binary(value), do: value
  def unwrap!(value) when is_list(value), do: value
end
