defmodule Emerge.Engine.VNode do
  @moduledoc """
  Internal virtual node that keeps identity and keys for reconciliation.
  """

  @type t :: %__MODULE__{
          id: term(),
          kind: atom(),
          key: term() | nil,
          attrs: map(),
          children: [t()],
          nearby: %{optional(atom()) => t()}
        }

  defstruct [:id, :kind, :key, :attrs, children: [], nearby: %{}]
end
