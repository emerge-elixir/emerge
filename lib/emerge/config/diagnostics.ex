defmodule Emerge.Config.Diagnostics do
  @moduledoc """
  Optional renderer measurements and logging. Set this record as
  `Emerge.Config.diagnostics`. It does not create an on-screen overlay.

  ```elixir
  %Emerge.Config{
    diagnostics: %Emerge.Config.Diagnostics{logs: [:stats]}
  }
  ```

  ## Fields

  - `stats` — `false` by default. With `true`, collect counters/timings for
    `EmergeSkia.stats/2` without printing periodic summaries.
  - `logs` — opt-in log categories; defaults to `[]`. See the categories below.
    `:stats` enables both collection and printed summaries, even if `stats` is false.
  - `force_gpu_finish` — `false` by default. A DRM OpenGL diagnostic that adds
    blocking GPU waits around presentation. It can severely slow rendering and
    distort measurements; leave it off for normal use.

  All fields require a new viewport to change.

  ## Read measurements or print them

  To read measurements yourself, use `stats: true` and leave `:stats` out of `logs`:

  ```elixir
  %Emerge.Config{
    diagnostics: %Emerge.Config.Diagnostics{stats: true}
  }
  ```

  After starting the viewport:

  ```elixir
  renderer = Emerge.renderer(viewport)
  {:ok, snapshot} = EmergeSkia.stats(renderer, :peek)
  ```

  `:peek` reads without resetting; `:take` reads and resets the measurement window;
  `:reset` discards it. Available counters depend on the backend. Collection cannot
  be enabled later through the stats API.

  For printed summaries, use `logs: [:stats]`; setting `stats: true` as well is
  unnecessary. Summaries are logged at info level about every five seconds.
  Native summaries reset the same window read by the stats API, so choose one
  approach when you need predictable measurement intervals.

  macOS supports printed summaries but not `EmergeSkia.stats/2`. Its summaries are
  cumulative; `stats: true` alone has no effect there.

  ## Log categories

  - `:input` — input-device and input-processing traces. It does not subscribe
    your viewport to input callbacks.
  - `:render` — tree processing, drawing and presentation traces; potentially verbose.
  - `:close` — window-close handling traces.
  - `:stats` — periodic performance summaries, also enabling collection.
  - `:animation` — Wayland animation/pacing traces.

  Categories are independent, not verbosity levels. Only `:stats` enables collection.
  The macOS host currently ignores `:input`, `:render` and `:animation`.
  `logs: []` disables extra traces, not ordinary warnings or errors.

  Viewports forward logs to Elixir `Logger`. With standalone `EmergeSkia.start/1`,
  the starting process receives `{:emerge_skia_log, level, source, message}`;
  use `EmergeSkia.set_log_target/2` to choose another recipient. Some low-level
  traces go directly to stderr and do not obey Logger filtering or redirection.

  ## Forced GPU waits

  `force_gpu_finish` affects DRM OpenGL and raster GPU-upload presentation only.
  It has no effect on CPU raster, Wayland or macOS; DRM Vulkan rejects it.
  It does not enable measurements/logs and is not a production synchronization fix.
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
