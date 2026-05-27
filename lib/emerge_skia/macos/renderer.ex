defmodule EmergeSkia.Macos.Renderer do
  @moduledoc false

  @enforce_keys [:session_id, :host_id, :host_pid, :backend_renderer]
  defstruct [:session_id, :host_id, :host_pid, :backend_renderer]

  @type t :: %__MODULE__{
          session_id: pos_integer(),
          host_id: non_neg_integer(),
          host_pid: non_neg_integer(),
          backend_renderer: :metal | :raster
        }
end
