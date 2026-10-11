defmodule Emerge.Runtime.Viewport.State do
  @moduledoc false

  @enforce_keys [:module]
  defstruct module: nil,
            renderer: nil,
            config: nil,
            renderer_config: nil,
            config_error: nil,
            config_pending: nil,
            config_applied: nil,
            lifecycle: nil,
            lifecycle_supervisor: nil,
            renderer_generation: nil,
            retiring_renderer_generation: nil,
            renderer_relay: nil,
            renderer_bound?: false,
            diff_state: nil,
            dirty?: false,
            flush_scheduled?: false,
            last_renderer_heartbeat_at_ms: nil,
            renderer_module: Emerge.Runtime.Viewport.Renderer.Skia,
            renderer_opts: [],
            skia_opts: [],
            input_mask: nil,
            renderer_check_interval_ms: 500

  @type t :: %__MODULE__{
          module: module(),
          renderer: term() | nil,
          config: Emerge.Config.normalized_t() | nil,
          renderer_config: Emerge.Config.normalized_t() | nil,
          config_error: {:error, term()} | nil,
          config_pending: {non_neg_integer(), pos_integer()} | nil,
          config_applied: {non_neg_integer(), pos_integer()} | nil,
          lifecycle: pid() | nil,
          lifecycle_supervisor: pid() | nil,
          renderer_generation: reference() | nil,
          retiring_renderer_generation: reference() | nil,
          renderer_relay: pid() | nil,
          renderer_bound?: boolean(),
          diff_state: Emerge.Engine.diff_state() | nil,
          dirty?: boolean(),
          flush_scheduled?: boolean(),
          last_renderer_heartbeat_at_ms: integer() | nil,
          renderer_module: module(),
          renderer_opts: keyword(),
          skia_opts: keyword(),
          input_mask: non_neg_integer() | nil,
          renderer_check_interval_ms: non_neg_integer() | nil
        }
end
