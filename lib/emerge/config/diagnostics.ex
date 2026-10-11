defmodule Emerge.Config.Diagnostics do
  @moduledoc """
  Controls diagnostic data collection and extra trace output at renderer startup.

  **`stats` collects data; `logs` requests output.** Neither creates an on-screen
  overlay. All fields are startup-only: changing them requires a new viewport.

  ## Fields and defaults

  | Field | Default | Effect |
  |---|---|---|
  | `stats` | `false` | Collect renderer counters and timings for `EmergeSkia.stats/2`, without enabling periodic stats logs. |
  | `logs` | `[]` | Enable the extra trace categories listed below. `:stats` also enables stats collection. |
  | `force_gpu_finish` | `false` | Add blocking GPU waits before and after each DRM EGL buffer swap; see the warning below. |

  ## Stats versus stats logs

  On native Wayland, DRM and headless renderers:

  | `stats` | `:stats` in `logs`? | Result |
  |---|---|---|
  | `false` | No | No collection or periodic stats output; the stats API reports `enabled: false`. |
  | `true` | No | Collect data for the stats API, but do not print periodic summaries. |
  | `false` | Yes | Collect data **and** emit periodic summaries. |
  | `true` | Yes | Same as the previous row; `stats: true` is redundant here. |

  Collected data includes frame counts, layout/render/presentation timings and
  cache counters. Available metrics depend on the backend and rendering API.
  Collection adds measurement overhead.

  For measurements you read yourself, use:

      %Emerge.Config{diagnostics: %Emerge.Config.Diagnostics{stats: true}}

  After the viewport has acquired its renderer:

      renderer = Emerge.renderer(viewport)
      {:ok, snapshot} = EmergeSkia.stats(renderer, :peek)

  `:peek` reads without resetting; `:take` reads and resets the measurement window.
  Stats collection cannot be switched on or off through `EmergeSkia.stats/2` after
  renderer startup.

  For automatic summaries instead, use:

      %Emerge.Config{diagnostics: %Emerge.Config.Diagnostics{logs: [:stats]}}

  Summaries use the `renderer_stats` log source at info level, approximately every
  five seconds. Pending asynchronous GPU measurements can delay a summary. Native
  stats logging reads and resets the **same measurement window** as the stats API,
  so leave `:stats` out of `logs` when you need to control window boundaries yourself.

  The macOS host currently supports `logs: [:stats]`, but not `EmergeSkia.stats/2`
  (`{:error, :not_supported}`). Its periodic summaries are cumulative rather than
  resetting windows. `stats: true` alone is not currently forwarded to that host.

  ## Log categories

  These categories are independent switches, not verbosity levels:

  | Category | Extra output |
  |---|---|
  | `:input` | DRM input-device discovery/add/remove details, cursor-motion enqueue traces and input-queue/poll problems. It does not subscribe your viewport to input events. |
  | `:render` | Native tree/event processing and frame drawing/presentation traces. Can be very verbose. |
  | `:close` | Wayland close/unmap/delivery traces and viewport close handling. |
  | `:stats` | Periodic renderer performance summaries, as described above. |
  | `:animation` | Wayland animation scene, pulse, draw and frame-callback traces; useful for investigating pacing. |

  `:input`, `:render` and `:animation` have no effect on the macOS host currently.
  Categories other than `:stats` do not enable stats collection. `logs: []` disables
  these opt-in traces, **not** ordinary startup, warning or error messages.

  Relayed native messages arrive as `{:emerge_skia_log, level, source, message}`.
  An Emerge viewport forwards these to Elixir `Logger`; its level/filter settings
  still apply. When using `EmergeSkia.start/1` directly, the starting process receives
  the messages; use `EmergeSkia.set_log_target/2` to choose another recipient.
  Some low-level input, render and close traces go directly to native stderr and
  cannot be redirected with `set_log_target/2` or filtered by Elixir `Logger`.

  ## Forced GPU completion

  `force_gpu_finish: true` calls OpenGL `glFinish` before **and** after each
  `eglSwapBuffers` in the DRM EGL presenter. These calls block the presenting
  thread until submitted GPU work finishes. This applies to DRM OpenGL and raster
  GPU-upload presentation, not CPU raster, Wayland or macOS. Explicit DRM Vulkan
  rejects the option.

  Use it only to investigate driver/presentation problems: it reduces CPU/GPU
  overlap, can severely slow rendering and changes the timings you are measuring.
  It does not enable stats collection or periodic stats logs, and is not a normal
  synchronization setting or a production fix. Normal safe presenter teardown
  performs its required GPU completion independently of this option.
  """
  defstruct stats: false,
            logs: [],
            force_gpu_finish: false

  @typedoc "Opt-in trace categories; `:stats` also enables collection, unlike the other categories."
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
