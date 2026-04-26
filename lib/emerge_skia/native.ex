defmodule EmergeSkia.Native do
  @moduledoc """
  NIF bindings for the Skia renderer.
  """

  @version Mix.Project.config()[:version]
  @base_url {EmergeSkia.BuildConfig, :precompiled_tar_gz_url}
  @precompiled_targets EmergeSkia.BuildConfig.precompiled_targets()
  @precompiled_nif_versions EmergeSkia.BuildConfig.precompiled_nif_versions()
  @checksum_only EmergeSkia.BuildConfig.checksum_only_mode?()
  @load_native_runtime EmergeSkia.BuildConfig.load_native_runtime?()

  if @checksum_only do
    # Checksum generation only needs RustlerPrecompiled metadata, not a built or downloaded NIF.
    :ok =
      EmergeSkia.ChecksumMetadata.ensure_written!(
        __MODULE__,
        otp_app: :emerge,
        crate: "emerge_skia",
        base_url: @base_url,
        version: @version,
        targets: @precompiled_targets,
        nif_versions: @precompiled_nif_versions,
        variants: EmergeSkia.BuildConfig.precompiled_variants()
      )
  else
    @rustler_opts Mix.Project.config()[:rustler_opts] || []
    @crate_path Path.expand("../../native/emerge_skia", __DIR__)
    @compiled_backends EmergeSkia.BuildConfig.compiled_backends()
    @checksum_path Path.expand("../../checksum-Elixir.EmergeSkia.Native.exs", __DIR__)
    @precompiled_variants EmergeSkia.BuildConfig.precompiled_variants()
    @cargo_features EmergeSkia.BuildConfig.compiled_backends_to_rustler_features(
                      @compiled_backends
                    )
    @force_build EmergeSkia.BuildConfig.force_precompiled_build?(
                   checksum_path: @checksum_path,
                   compiled_backends: @compiled_backends,
                   targets: @precompiled_targets,
                   nif_versions: @precompiled_nif_versions
                 )

    if @load_native_runtime do
      use RustlerPrecompiled,
          Keyword.merge(
            [
              otp_app: :emerge,
              crate: "emerge_skia",
              base_url: @base_url,
              version: @version,
              force_build: @force_build,
              targets: @precompiled_targets,
              nif_versions: @precompiled_nif_versions,
              variants: @precompiled_variants,
              path: @crate_path,
              default_features: false,
              features: @cargo_features
            ],
            @rustler_opts
          )
    end
  end

  @doc """
  Start the Skia renderer with a window.

  Returns a renderer resource that can be used with other functions.
  """
  @spec start(String.t(), non_neg_integer(), non_neg_integer()) ::
          reference() | {:ok, reference()} | {:error, term()}
  def start(_title, _width, _height), do: :erlang.nif_error(:nif_not_loaded)

  @doc """
  Start the Skia renderer with backend options.

  Mirrors `EmergeSkia.start/1` keyword options.
  """
  @spec start_opts(%{
          required(:backend) => String.t(),
          required(:title) => String.t(),
          required(:width) => non_neg_integer(),
          required(:height) => non_neg_integer(),
          required(:drm_card) => String.t() | nil,
          required(:drm_startup_retries) => non_neg_integer(),
          required(:drm_retry_interval_ms) => non_neg_integer(),
          required(:asset_sources) => [String.t()],
          required(:asset_runtime_enabled) => boolean(),
          required(:asset_allowlist) => [String.t()],
          required(:asset_follow_symlinks) => boolean(),
          required(:asset_max_file_size) => pos_integer(),
          required(:asset_extensions) => [String.t()],
          required(:drm_cursor) => [
            %{
              required(:icon) => String.t(),
              required(:source) => String.t(),
              required(:hotspot_x) => float(),
              required(:hotspot_y) => float()
            }
          ],
          required(:scroll_line_pixels) => float(),
          required(:hw_cursor) => boolean(),
          required(:input_log) => boolean(),
          required(:render_log) => boolean(),
          required(:close_signal_log) => boolean(),
          required(:stats_enabled) => boolean(),
          required(:renderer_stats_log) => boolean()
        }) :: reference() | {:ok, reference()} | {:error, term()}
  def start_opts(_opts), do: :erlang.nif_error(:nif_not_loaded)

  @doc """
  Stop the renderer and close the window.
  """
  @spec stop(reference()) :: :ok
  def stop(_renderer), do: :erlang.nif_error(:nif_not_loaded)

  @doc """
  Upload a full EMRG tree, run layout, and render immediately.
  Window dimensions come from the initial start config and resize events.
  """
  @spec renderer_upload(reference(), binary()) :: :ok
  def renderer_upload(_renderer, _data), do: :erlang.nif_error(:nif_not_loaded)

  @doc """
  Apply EMRG patches, run layout, and render immediately.
  Window dimensions come from the initial start config and resize events.
  """
  @spec renderer_patch(reference(), binary()) :: :ok
  def renderer_patch(_renderer, _data), do: :erlang.nif_error(:nif_not_loaded)

  @doc """
  Measure text dimensions.

  Returns `{width, line_height, ascent, descent}`.
  """
  @spec measure_text(String.t(), float()) :: {float(), float(), float(), float()}
  def measure_text(_text, _font_size), do: :erlang.nif_error(:nif_not_loaded)

  @doc """
  Load a font from binary data and register it with a name.

  ## Parameters
  - `name` - Font family name to register (e.g., "my-font")
  - `weight` - Font weight (100-900, 400=normal, 700=bold)
  - `italic` - Whether this is an italic variant
  - `data` - Binary font data (TTF file contents)

  ## Example
      {:ok, data} = File.read("fonts/MyFont-Bold.ttf")
      {:ok, true} = EmergeSkia.Native.load_font_nif("my-font", 700, false, data)
  """
  @spec load_font_nif(String.t(), non_neg_integer(), boolean(), binary()) ::
          {:ok, boolean()} | {:error, String.t()}
  def load_font_nif(_name, _weight, _italic, _data), do: :erlang.nif_error(:nif_not_loaded)

  @doc """
  Configure native asset loading policy and source roots.
  """
  @spec configure_assets_nif(
          reference(),
          [String.t()],
          boolean(),
          [String.t()],
          boolean(),
          non_neg_integer(),
          [String.t()]
        ) :: :ok
  def configure_assets_nif(
        _renderer,
        _sources,
        _runtime_enabled,
        _allowlist,
        _follow_symlinks,
        _max_file_size,
        _extensions
      ),
      do: :erlang.nif_error(:nif_not_loaded)

  @doc """
  Check if the renderer is still running.
  """
  @spec is_running(reference()) :: boolean()
  def is_running(_renderer), do: :erlang.nif_error(:nif_not_loaded)

  @doc """
  Create a renderer-owned video target.
  """
  @spec video_target_new(reference(), String.t(), pos_integer(), pos_integer(), String.t()) ::
          reference() | {:ok, reference()} | {:error, String.t()}
  def video_target_new(_renderer, _id, _width, _height, _mode),
    do: :erlang.nif_error(:nif_not_loaded)

  @doc """
  Submit a DRM Prime descriptor to a video target.
  """
  @spec video_target_submit_prime(reference(), map()) ::
          {:ok, boolean()} | {:error, String.t()}
  def video_target_submit_prime(_target, _desc), do: :erlang.nif_error(:nif_not_loaded)

  # ===========================================================================
  # Raster Backend
  # ===========================================================================

  @doc """
  Render a tree to an RGBA pixel buffer (synchronous, no window).

  The tree is provided as an encoded EMRG binary. Asset policy mirrors
  `EmergeSkia.start/1`, with an additional offscreen asset mode.
  """
  @spec render_tree_to_pixels_nif(
          binary(),
          map()
        ) :: binary() | {:ok, binary()} | {:error, String.t()}
  def render_tree_to_pixels_nif(_data, _opts),
    do: :erlang.nif_error(:nif_not_loaded)

  @doc """
  Render a tree to an encoded PNG binary (synchronous, no window).

  The tree is provided as an encoded EMRG binary. Asset policy mirrors
  `EmergeSkia.start/1`, with an additional offscreen asset mode.
  """
  @spec render_tree_to_png_nif(
          binary(),
          map()
        ) :: binary() | {:ok, binary()} | {:error, String.t()}
  def render_tree_to_png_nif(_data, _opts),
    do: :erlang.nif_error(:nif_not_loaded)

  # ===========================================================================
  # Input Handling
  # ===========================================================================

  @doc """
  Set the input event mask to filter which events are sent.

  Mask bits:
  - 0x01: Key events
  - 0x02: Codepoint (text input) events
  - 0x04: Cursor position events
  - 0x08: Cursor button events
  - 0x10: Cursor scroll events
  - 0x20: Cursor enter/exit events
  - 0x40: Resize events
  - 0x80: Focus events
  - 0xFF: All events
  """
  @spec set_input_mask(reference(), non_neg_integer()) :: :ok
  def set_input_mask(_renderer, _mask), do: :erlang.nif_error(:nif_not_loaded)

  @doc """
  Set the target process to receive input events.

  Input events are sent directly to the target process as
  `{:emerge_skia_event, event}` messages.
  """
  @spec set_input_target(reference(), pid() | nil) :: :ok
  def set_input_target(_renderer, _pid), do: :erlang.nif_error(:nif_not_loaded)

  @doc """
  Set the target process to receive native renderer log messages.

  Native logs are sent directly to the target process as
  `{:emerge_skia_log, level, source, message}` messages.
  """
  @spec set_log_target(reference(), pid() | nil) :: :ok
  def set_log_target(_renderer, _pid), do: :erlang.nif_error(:nif_not_loaded)

  @type stats_command ::
          :peek | :take | :reset | {:configure, %{required(:enabled) => boolean()}}

  @type duration_stats :: %{
          required(:count) => non_neg_integer(),
          required(:avg_ms) => float(),
          required(:min_ms) => float(),
          required(:max_ms) => float()
        }

  @type layout_cache_stats :: %{
          required(:intrinsic_measure_hits) => non_neg_integer(),
          required(:intrinsic_measure_misses) => non_neg_integer(),
          required(:intrinsic_measure_stores) => non_neg_integer(),
          required(:intrinsic_measure_ineligible_bypasses) => non_neg_integer(),
          required(:subtree_measure_hits) => non_neg_integer(),
          required(:subtree_measure_misses) => non_neg_integer(),
          required(:subtree_measure_stores) => non_neg_integer(),
          required(:subtree_measure_dirty_bypasses) => non_neg_integer(),
          required(:subtree_measure_animation_bypasses) => non_neg_integer(),
          required(:resolve_hits) => non_neg_integer(),
          required(:resolve_misses) => non_neg_integer(),
          required(:resolve_stores) => non_neg_integer(),
          required(:resolve_dirty_bypasses) => non_neg_integer(),
          required(:resolve_ineligible_bypasses) => non_neg_integer(),
          required(:resolve_animation_bypasses) => non_neg_integer(),
          required(:resolve_store_bypasses) => non_neg_integer()
        }

  @type stats_snapshot :: %{
          required(:version) => pos_integer(),
          required(:kind) => String.t(),
          required(:enabled) => boolean(),
          required(:window) => %{
            required(:elapsed_ms) => non_neg_integer(),
            required(:reset_on_read) => boolean()
          },
          required(:frames) => %{
            required(:fps) => float(),
            required(:display_fps) => float(),
            required(:display_frame_ms) => float(),
            required(:frame_count) => non_neg_integer()
          },
          required(:timings) => %{
            required(:render) => duration_stats(),
            required(:present_submit) => duration_stats(),
            required(:layout) => duration_stats(),
            required(:refresh) => duration_stats(),
            required(:event_resolve) => duration_stats(),
            required(:patch_tree_process) => duration_stats()
          },
          required(:counters) => %{
            required(:layout_cache) => layout_cache_stats()
          }
        }

  @doc false
  @spec stats(reference(), stats_command()) :: {:ok, stats_snapshot()} | {:error, String.t()}
  def stats(_resource, _command), do: :erlang.nif_error(:nif_not_loaded)

  # ===========================================================================
  # Tree Functions (Emerge Integration)
  # ===========================================================================

  @doc """
  Create a new empty tree resource.
  """
  @spec tree_new() :: reference()
  def tree_new, do: :erlang.nif_error(:nif_not_loaded)

  @doc """
  Upload a full tree from EMRG binary format.
  Replaces any existing tree contents.
  """
  @spec tree_upload(reference(), binary()) :: {:ok, boolean()} | {:error, String.t()}
  def tree_upload(_tree, _data), do: :erlang.nif_error(:nif_not_loaded)

  @doc """
  Upload a full tree and return the encoded EMRG binary.
  """
  @spec tree_upload_roundtrip(reference(), binary()) ::
          binary() | {:ok, binary()} | {:error, String.t()}
  def tree_upload_roundtrip(_tree, _data), do: :erlang.nif_error(:nif_not_loaded)

  @doc """
  Apply patches to an existing tree.
  """
  @spec tree_patch(reference(), binary()) :: {:ok, boolean()} | {:error, String.t()}
  def tree_patch(_tree, _data), do: :erlang.nif_error(:nif_not_loaded)

  @doc """
  Apply patches to an existing tree and return the encoded EMRG binary.
  """
  @spec tree_patch_roundtrip(reference(), binary()) ::
          binary() | {:ok, binary()} | {:error, String.t()}
  def tree_patch_roundtrip(_tree, _data), do: :erlang.nif_error(:nif_not_loaded)

  @doc """
  Get the number of nodes in the tree.
  """
  @spec tree_node_count(reference()) :: non_neg_integer()
  def tree_node_count(_tree), do: :erlang.nif_error(:nif_not_loaded)

  @doc """
  Check if the tree is empty.
  """
  @spec tree_is_empty(reference()) :: boolean()
  def tree_is_empty(_tree), do: :erlang.nif_error(:nif_not_loaded)

  @doc """
  Clear the tree.
  """
  @spec tree_clear(reference()) :: :ok
  def tree_clear(_tree), do: :erlang.nif_error(:nif_not_loaded)

  @doc """
  Compute layout for the tree with given width/height constraints and scale factor.

  Returns a list of `{id_binary, x, y, width, height}` tuples for all elements.
  The `id_binary` is the element `id` encoded as `<<id::unsigned-big-64>>`.

  Scale is applied to all pixel-based attributes (px sizes, padding, spacing,
  border radius, border width, font size). Use scale > 1.0 for high-DPI displays.
  """
  @spec tree_layout(reference(), float(), float(), float()) ::
          {:ok, list({binary(), float(), float(), float(), float()})} | {:error, String.t()}
  def tree_layout(_tree, _width, _height, _scale), do: :erlang.nif_error(:nif_not_loaded)

  @doc """
  Decode an EMRG binary in Rust and re-encode it.
  """
  @spec tree_roundtrip(binary()) :: binary() | {:error, String.t()}
  def tree_roundtrip(_data), do: :erlang.nif_error(:nif_not_loaded)

  @doc false
  @spec test_harness_new(pos_integer(), pos_integer()) ::
          reference() | {:ok, reference()} | {:error, String.t()}
  def test_harness_new(_width, _height), do: :erlang.nif_error(:nif_not_loaded)

  @doc false
  @spec test_harness_upload(reference(), binary()) :: :ok
  def test_harness_upload(_harness, _data), do: :erlang.nif_error(:nif_not_loaded)

  @doc false
  @spec test_harness_patch(reference(), binary()) :: :ok
  def test_harness_patch(_harness, _data), do: :erlang.nif_error(:nif_not_loaded)

  @doc false
  @spec test_harness_cursor_pos(reference(), number(), number()) ::
          :ok
  def test_harness_cursor_pos(_harness, _x, _y), do: :erlang.nif_error(:nif_not_loaded)

  @doc false
  @spec test_harness_animation_pulse(reference(), non_neg_integer(), non_neg_integer()) ::
          {:ok, boolean()} | {:error, String.t()}
  def test_harness_animation_pulse(_harness, _presented_ms, _predicted_ms),
    do: :erlang.nif_error(:nif_not_loaded)

  @doc false
  @spec test_harness_reset_clock(reference()) :: :ok
  def test_harness_reset_clock(_harness), do: :erlang.nif_error(:nif_not_loaded)

  @doc false
  @spec test_harness_await_render(reference(), non_neg_integer()) ::
          {:ok, boolean()} | {:error, String.t()}
  def test_harness_await_render(_harness, _timeout_ms), do: :erlang.nif_error(:nif_not_loaded)

  @doc false
  @spec test_harness_drain_mouse_over_msgs(reference(), non_neg_integer()) ::
          [{binary(), boolean()}]
  def test_harness_drain_mouse_over_msgs(_harness, _timeout_ms),
    do: :erlang.nif_error(:nif_not_loaded)

  @doc false
  @spec test_harness_stop(reference()) :: :ok
  def test_harness_stop(_harness), do: :erlang.nif_error(:nif_not_loaded)
end
