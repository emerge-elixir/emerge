defmodule Emerge.Config.Runtime do
  @moduledoc """
  Renderer health checks, DRM startup retries and custom renderer selection.
  Set this record as `Emerge.Config.runtime`. Most applications should keep the
  defaults.

  ```elixir
  %Emerge.Config{
    runtime: %Emerge.Config.Runtime{health_check_interval_ms: 1_000}
  }
  ```

  ## Fields

  - `health_check_interval_ms` — milliseconds between renderer checks;
    defaults to `500`. For the standard renderer, `nil` uses 500 ms and `0`
    schedules immediate checks; prefer a positive interval. For custom adapters,
    `nil` or `0` disables viewport polling of `running?/1`.
  - `startup_retries` — retries allowed when initially setting up a DRM display;
    defaults to `40`. `0` allows only the initial attempt. This is separate from
    the viewport's automatic recovery after a renderer stops.
  - `retry_interval_ms` — milliseconds between those DRM startup attempts;
    defaults to `250`. `0` adds no retry delay. This does not extend the renderer's
    startup timeout.
  - `adapter` — renderer adapter module. Defaults to the standard Skia adapter.
    Only change this when providing your own implementation.
  - `adapter_opts` — opaque keyword options passed to that adapter; defaults to `[]`.
    These are not renderer configuration fields.

  ## Changing settings

  Only `health_check_interval_ms` can change through `config`. Changing retry
  policy, adapter or adapter options requires a new viewport.
  """
  defstruct health_check_interval_ms: 500,
            startup_retries: 40,
            retry_interval_ms: 250,
            adapter: Emerge.Runtime.Viewport.Renderer.Skia,
            adapter_opts: []

  @type t :: %__MODULE__{
          health_check_interval_ms: non_neg_integer() | nil,
          startup_retries: non_neg_integer(),
          retry_interval_ms: non_neg_integer(),
          adapter: module(),
          adapter_opts: keyword()
        }
  @typedoc "Typed keyword shorthand accepted in this record's parent field."
  @type options :: [
          {:health_check_interval_ms, non_neg_integer() | nil}
          | {:startup_retries, non_neg_integer()}
          | {:retry_interval_ms, non_neg_integer()}
          | {:adapter, module()}
          | {:adapter_opts, keyword()}
        ]
  @typedoc "Canonical form after nested keyword records have been expanded."
  @type normalized_t :: t()

  @doc false
  def __schema__,
    do: [
      health_check_interval_ms: {:optional, :non_negative_integer},
      startup_retries: :non_negative_integer,
      retry_interval_ms: :non_negative_integer,
      adapter: :module,
      adapter_opts: :keyword
    ]
end
