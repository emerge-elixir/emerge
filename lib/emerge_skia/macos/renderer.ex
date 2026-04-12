defmodule EmergeSkia.Macos.Renderer do
  @moduledoc false

  @enforce_keys [:session_id, :host_id, :host_pid, :macos_backend]
  defstruct [:session_id, :host_id, :host_pid, :macos_backend]

  @type t :: %__MODULE__{
          session_id: pos_integer(),
          host_id: non_neg_integer(),
          host_pid: non_neg_integer(),
          macos_backend: :metal | :raster
        }
end
