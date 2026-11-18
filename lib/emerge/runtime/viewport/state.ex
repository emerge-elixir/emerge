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
            config_accepted: nil,
            lifecycle: nil,
            lifecycle_supervisor: nil,
            renderer_generation: nil,
            retiring_renderer_generation: nil,
            renderer_relay: nil,
            renderer_bound?: false,
            diff_state: nil,
            drawn_routes: nil,
            pending_routes: nil,
            submitting_routes: nil,
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
          config_accepted: {non_neg_integer(), pos_integer()} | nil,
          lifecycle: pid() | nil,
          lifecycle_supervisor: pid() | nil,
          renderer_generation: reference() | nil,
          retiring_renderer_generation: reference() | nil,
          renderer_relay: pid() | nil,
          renderer_bound?: boolean(),
          diff_state: Emerge.Engine.diff_state() | nil,
          drawn_routes: map() | nil,
          pending_routes: map() | nil,
          submitting_routes: map() | nil,
          dirty?: boolean(),
          flush_scheduled?: boolean(),
          last_renderer_heartbeat_at_ms: integer() | nil,
          renderer_module: module(),
          renderer_opts: keyword(),
          skia_opts: keyword(),
          input_mask: non_neg_integer() | nil,
          renderer_check_interval_ms: non_neg_integer() | nil
        }

  # Only one submission may be sealed. Edits after sealing inherit from its
  # persistent map, not the addresses of the still-visible frame.
  def routes(rt, {:edit, edits}) do
    base = rt.pending_routes || rt.submitting_routes || rt.drawn_routes || %{}

    next =
      Enum.reduce(edits, base, fn
        {id, nil}, routes -> Map.delete(routes, id)
        {id, events}, routes -> Map.put(routes, id, events)
      end)

    %{rt | pending_routes: next}
  end

  def routes(rt, :seal), do: %{rt | submitting_routes: rt.pending_routes, pending_routes: nil}

  def routes(rt, :publish),
    do: %{rt | drawn_routes: rt.submitting_routes || rt.drawn_routes, submitting_routes: nil}

  def routes(rt, :retry),
    do: %{rt | pending_routes: rt.pending_routes || rt.submitting_routes, submitting_routes: nil}

  def routes(rt, :install),
    do: %{rt | drawn_routes: rt.pending_routes || rt.drawn_routes, pending_routes: nil}
end
