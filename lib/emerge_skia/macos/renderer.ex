defmodule EmergeSkia.Macos.Renderer do
  @moduledoc false

  @enforce_keys [
    :session_id,
    :host_id,
    :host_pid,
    :requested_rendering_api,
    :rendering_api,
    :renderer_cache_enabled
  ]
  defstruct [
    :session_id,
    :host_id,
    :host_pid,
    :requested_rendering_api,
    :rendering_api,
    :renderer_cache_enabled
  ]

  @type rendering_api :: :auto | :metal | :raster

  @type t :: %__MODULE__{
          session_id: pos_integer(),
          host_id: non_neg_integer(),
          host_pid: non_neg_integer(),
          requested_rendering_api: rendering_api(),
          rendering_api: :metal | :raster,
          renderer_cache_enabled: boolean()
        }
end
