defmodule Emerge.Runtime.Viewport.State do
  @moduledoc false

  @enforce_keys [:module, :mount_opts]
  defstruct module: nil,
            mount_opts: [],
            user_state: nil,
            renderer: nil,
            diff_state: nil,
            dirty?: false,
            flush_scheduled?: false,
            renderer_module: Emerge.Runtime.Viewport.Renderer.Skia,
            renderer_opts: [],
            skia_opts: [],
            input_mask: nil,
            renderer_check_interval_ms: 500

  @type t :: %__MODULE__{
          module: module(),
          mount_opts: keyword(),
          user_state: term(),
          renderer: term() | nil,
          diff_state: Emerge.Engine.diff_state() | nil,
          dirty?: boolean(),
          flush_scheduled?: boolean(),
          renderer_module: module(),
          renderer_opts: keyword(),
          skia_opts: keyword(),
          input_mask: non_neg_integer() | nil,
          renderer_check_interval_ms: non_neg_integer() | nil
        }
end
