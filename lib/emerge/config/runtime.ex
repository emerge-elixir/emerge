defmodule Emerge.Config.Runtime do
  @moduledoc """
  Advanced renderer health, DRM startup retry policy and custom adapter configuration. Normally leave these defaults unchanged.

  ## Fields and defaults

  | Field | Default |
  |---|---|
  | `health_check_interval_ms` | `500` |
  | `startup_retries` | `40` |
  | `retry_interval_ms` | `250` |
  | `adapter` | `Emerge.Runtime.Viewport.Renderer.Skia` |
  | `adapter_opts` | `[]` |
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
