defmodule Emerge.Assets.Ref do
  @moduledoc """
  Compile-time verified asset path reference returned by `~m`.
  """

  @enforce_keys [:path]
  defstruct [:path, verified?: false]

  @type t :: %__MODULE__{
          path: String.t(),
          verified?: boolean()
        }

  @spec new(String.t(), keyword()) :: t()
  def new(path, opts \\ []) when is_binary(path) do
    %__MODULE__{path: path, verified?: Keyword.get(opts, :verified?, false)}
  end
end
