defmodule Emerge.Engine.VNode do
  @moduledoc """
  Internal virtual node that keeps identity and keys for reconciliation.
  """

  @type t :: %__MODULE__{
          id: non_neg_integer(),
          kind: atom(),
          key: term() | nil,
          owner: pos_integer() | nil,
          own: pos_integer(),
          inherited: pos_integer(),
          native: binary() | {:media, binary(), binary(), binary()},
          stamp: <<_::64>>,
          index: map() | nil,
          attrs: map(),
          events: %{term() => {pid(), term()}},
          children: [t()],
          nearby: [{atom(), t()}]
        }

  # The accepted root alone owns the lookup index; its indexed record has
  # index: nil, so retaining a new generation cannot pin previous indexes.
  defstruct [
    :id,
    :kind,
    :key,
    :attrs,
    :owner,
    :own,
    :inherited,
    :native,
    :stamp,
    :index,
    events: %{},
    children: [],
    nearby: []
  ]
end
