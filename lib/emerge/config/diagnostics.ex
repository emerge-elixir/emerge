defmodule Emerge.Config.Diagnostics do
  @moduledoc """
  Opt-in statistics and log categories. `force_gpu_finish` is an advanced DRM/OpenGL diagnostic, not a normal synchronization option.

  ## Fields and defaults

  | Field | Default |
  |---|---|
  | `stats` | `false` |
  | `logs` | `[]` |
  | `force_gpu_finish` | `false` |
  """
  defstruct stats: false,
            logs: [],
            force_gpu_finish: false

  @typedoc "Input discovery, rendering, close handling, periodic stats, or animation traces."
  @type log_category :: :input | :render | :close | :stats | :animation
  @type t :: %__MODULE__{stats: boolean(), logs: [log_category()], force_gpu_finish: boolean()}
  @typedoc "Typed keyword shorthand accepted in this record's parent field."
  @type options :: [
          {:stats, boolean()}
          | {:logs, [log_category()]}
          | {:force_gpu_finish, boolean()}
        ]
  @typedoc "Canonical form after nested keyword records have been expanded."
  @type normalized_t :: t()

  @doc false
  def __schema__,
    do: [
      stats: :boolean,
      logs: {:set, {:enum, [:input, :render, :close, :stats, :animation]}},
      force_gpu_finish: :boolean
    ]
end
