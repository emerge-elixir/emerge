defmodule EmergeSkia.VideoTarget do
  @moduledoc """
  Handle for a renderer-owned video target.

  `id` is serialized into the UI tree, while `ref` is passed to native submit APIs.
  """

  @enforce_keys [:id, :width, :height, :mode, :ref]
  defstruct [:id, :width, :height, :mode, :ref]

  @type mode :: :prime

  @type t :: %__MODULE__{
          id: String.t(),
          width: pos_integer(),
          height: pos_integer(),
          mode: mode(),
          ref: reference()
        }
end
