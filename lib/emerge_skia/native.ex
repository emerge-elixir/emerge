defmodule EmergeSkia.Native do
  @moduledoc """
  NIF bindings for the Skia renderer.
  """

  @behaviour VideoInterop.AbandonmentGuard

  @checksum_only EmergeSkia.BuildConfig.checksum_only_mode?()
  @load_native_runtime EmergeSkia.BuildConfig.load_native_runtime?()

  if @checksum_only do
    @version Mix.Project.config()[:version]
    @base_url {EmergeSkia.BuildConfig, :precompiled_tar_gz_url}
    @precompiled_targets EmergeSkia.BuildConfig.precompiled_targets()
    @precompiled_nif_versions EmergeSkia.BuildConfig.precompiled_nif_versions()

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
    if @load_native_runtime do
      @rustler_opts Mix.Project.config()[:rustler_opts] || []
      @crate_path Path.expand("../../native/emerge_skia", __DIR__)
      @compiled_backends EmergeSkia.BuildConfig.compiled_backends()
      @compiled_vulkan_backends EmergeSkia.BuildConfig.compiled_vulkan_backends()
      @checksum_path Path.expand("../../checksum-Elixir.EmergeSkia.Native.exs", __DIR__)
      @version Mix.Project.config()[:version]
      @base_url {EmergeSkia.BuildConfig, :precompiled_tar_gz_url}
      @precompiled_targets EmergeSkia.BuildConfig.precompiled_targets()
      @precompiled_nif_versions EmergeSkia.BuildConfig.precompiled_nif_versions()
      @precompiled_variants EmergeSkia.BuildConfig.precompiled_variants()
      @platform_cargo_features EmergeSkia.BuildConfig.rustler_platform_features(
                                 System.get_env(),
                                 @compiled_backends,
                                 @compiled_vulkan_backends
                               )
      @cargo_features EmergeSkia.BuildConfig.compiled_backends_to_rustler_features(
                        @compiled_backends,
                        @compiled_vulkan_backends
                      ) ++ @platform_cargo_features
      @force_build EmergeSkia.BuildConfig.force_precompiled_build?(
                     checksum_path: @checksum_path,
                     compiled_backends: @compiled_backends,
                     compiled_vulkan_backends: @compiled_vulkan_backends,
                     targets: @precompiled_targets,
                     nif_versions: @precompiled_nif_versions
                   )

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
  Start the Skia renderer with backend and rendering API options.

  Mirrors `EmergeSkia.start/1` keyword options.
  """
  @spec start_opts(%{
          required(:backend) => String.t(),
          required(:rendering_api) => %{
            required(:kind) => String.t(),
            required(:raster_present) => String.t(),
            required(:raster_present_configured) => boolean()
          },
          required(:title) => String.t(),
          required(:width) => non_neg_integer(),
          required(:height) => non_neg_integer(),
          required(:drm_card) => String.t() | nil,
          required(:vulkan_drm_node) => String.t() | nil,
          required(:drm_startup_retries) => non_neg_integer(),
          required(:drm_retry_interval_ms) => non_neg_integer(),
          required(:drm_force_gpu_finish) => boolean(),
          required(:asset_sources) => [String.t()],
          required(:asset_runtime_enabled) => boolean(),
          required(:asset_allowlist) => [String.t()],
          required(:asset_follow_symlinks) => boolean(),
          required(:asset_max_file_size) => pos_integer(),
          required(:asset_extensions) => [String.t()],
          required(:asset_cache_max_entries) => non_neg_integer(),
          required(:asset_cache_max_bytes) => non_neg_integer(),
          required(:asset_decode_at_size) => boolean(),
          required(:asset_memory_log) => boolean(),
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
          required(:renderer_stats_log) => boolean(),
          required(:renderer_animation_log) => boolean(),
          required(:renderer_cache) => %{
            required(:enabled) => boolean(),
            required(:max_new_payloads_per_frame) => non_neg_integer(),
            required(:paint_layer) => %{
              required(:max_entries) => non_neg_integer(),
              required(:max_bytes) => non_neg_integer(),
              required(:max_entry_bytes) => non_neg_integer(),
              required(:min_visible_before_store) => non_neg_integer(),
              required(:max_stale_frames) => non_neg_integer()
            }
          },
          required(:headless) => %{
            required(:target) => pid() | nil,
            required(:mode) => String.t(),
            required(:pixel_format) => String.t(),
            required(:bw1_polarity) => String.t(),
            required(:dither) => boolean(),
            required(:target_fps) => pos_integer() | nil,
            required(:frame_message) => String.t(),
            required(:prime) => %{
              required(:max_in_flight) => pos_integer(),
              required(:on_backpressure) => String.t()
            }
          }
        }) :: reference() | {:ok, reference()} | {:error, term()}
  def start_opts(_opts), do: :erlang.nif_error(:nif_not_loaded)

  @doc """
  Stop the renderer and close the window.
  """
  @spec stop(reference()) :: {:ok, :ok} | {:error, String.t()}
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
  @spec configure_assets_nif(reference(), map()) :: :ok
  def configure_assets_nif(_renderer, _opts), do: :erlang.nif_error(:nif_not_loaded)

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

  @type video_target_info :: %{
          required(:renderer_epoch) => non_neg_integer(),
          required(:target_id) => binary(),
          required(:target_incarnation) => non_neg_integer(),
          required(:active_stream_id) => non_neg_integer() | nil
        }

  @doc """
  Return the current exact identity of a renderer-owned video target.
  """
  @spec video_target_info(reference()) :: {:ok, video_target_info()} | {:error, String.t()}
  def video_target_info(_target), do: :erlang.nif_error(:nif_not_loaded)

  @doc """
  Submit a DRM Prime descriptor to a video target.
  """
  @spec video_target_submit_prime(reference(), map()) ::
          {:ok, boolean()} | {:error, String.t()}
  def video_target_submit_prime(_target, _desc), do: :erlang.nif_error(:nif_not_loaded)

  @doc false
  @spec video_consumer_session_open(reference(), VideoInterop.Format.t()) ::
          {:ok, reference()} | {:error, term()}
  def video_consumer_session_open(_target, _format),
    do: :erlang.nif_error(:nif_not_loaded)

  @doc false
  @spec video_consumer_session_submit(reference(), VideoInterop.Frame.t()) ::
          {:ok, :transferred | :released}
          | {:error, {:caller_owned | :transferred, term()}}
  def video_consumer_session_submit(_session, _frame), do: :erlang.nif_error(:nif_not_loaded)

  @doc false
  @spec video_consumer_decode_for_test(term()) ::
          {:ok, :caller_owned} | {:error, {:caller_owned, String.t()}}
  def video_consumer_decode_for_test(_frame), do: :erlang.nif_error(:nif_not_loaded)

  @doc false
  @spec video_consumer_prepare_hold_for_test(reference(), term()) ::
          {:ok, reference()} | {:error, {:caller_owned, String.t()}}
  def video_consumer_prepare_hold_for_test(_session, _frame),
    do: :erlang.nif_error(:nif_not_loaded)

  @doc false
  @spec video_consumer_prepared_drop_for_test(reference()) :: boolean()
  def video_consumer_prepared_drop_for_test(_prepared),
    do: :erlang.nif_error(:nif_not_loaded)

  @doc false
  @spec video_consumer_session_open_for_test() ::
          {:ok, {reference(), reference()}} | {:error, String.t()}
  def video_consumer_session_open_for_test, do: :erlang.nif_error(:nif_not_loaded)

  @doc false
  @spec video_consumer_target_set_active_for_test(reference(), boolean()) ::
          {:ok, boolean()} | {:error, String.t()}
  def video_consumer_target_set_active_for_test(_target, _active),
    do: :erlang.nif_error(:nif_not_loaded)

  @doc false
  @spec video_consumer_target_replace_for_test(reference()) ::
          {:ok, boolean()} | {:error, String.t()}
  def video_consumer_target_replace_for_test(_target),
    do: :erlang.nif_error(:nif_not_loaded)

  @doc false
  @spec video_consumer_target_pipeline_counts_for_test(reference()) ::
          {:ok, {non_neg_integer(), non_neg_integer(), non_neg_integer()}} | {:error, String.t()}
  def video_consumer_target_pipeline_counts_for_test(_target),
    do: :erlang.nif_error(:nif_not_loaded)

  @doc false
  @spec video_interop_open_fd_for_test() ::
          {:ok, {non_neg_integer(), reference()}} | {:error, String.t()}
  def video_interop_open_fd_for_test, do: :erlang.nif_error(:nif_not_loaded)

  @doc false
  @spec video_consumer_session_close(reference()) ::
          :ok | {:error, {:timeout | :dispatcher_close_failed, String.t()}}
  def video_consumer_session_close(_session), do: :erlang.nif_error(:nif_not_loaded)

  @doc false
  @spec video_consumer_session_close_with_timeout_for_test(reference(), non_neg_integer()) ::
          :ok | {:error, {:timeout | :dispatcher_close_failed, String.t()}}
  def video_consumer_session_close_with_timeout_for_test(_session, _timeout_ms),
    do: :erlang.nif_error(:nif_not_loaded)

  @doc false
  @spec headless_prime_release_dispatcher_new() ::
          {:ok, reference()} | {:error, String.t()}
  def headless_prime_release_dispatcher_new, do: :erlang.nif_error(:nif_not_loaded)

  @doc false
  @spec headless_prime_release_dispatcher_close(reference()) ::
          :ok | {:error, {:timeout | :dispatcher_close_failed, String.t()}}
  def headless_prime_release_dispatcher_close(_dispatcher),
    do: :erlang.nif_error(:nif_not_loaded)

  @doc false
  @spec headless_prime_release_dispatcher_close_with_timeout_for_test(
          reference(),
          non_neg_integer()
        ) :: :ok | {:error, {:timeout | :dispatcher_close_failed, String.t()}}
  def headless_prime_release_dispatcher_close_with_timeout_for_test(_dispatcher, _timeout_ms),
    do: :erlang.nif_error(:nif_not_loaded)

  @doc false
  @spec headless_prime_abandonment_guard_new(
          pid(),
          reference(),
          reference(),
          reference()
        ) :: {:ok, reference()} | {:error, String.t()}
  def headless_prime_abandonment_guard_new(_owner, _token, _holder, _dispatcher),
    do: :erlang.nif_error(:nif_not_loaded)

  @impl VideoInterop.AbandonmentGuard
  @spec video_interop_abandonment_guard?(term()) :: boolean()
  def video_interop_abandonment_guard?(_resource), do: :erlang.nif_error(:nif_not_loaded)

  @doc false
  @spec headless_prime_release_backend_token(reference()) :: :ok
  def headless_prime_release_backend_token(_backend_token),
    do: :erlang.nif_error(:nif_not_loaded)

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
          required(:subtree_measure_hits) => non_neg_integer(),
          required(:subtree_measure_misses) => non_neg_integer(),
          required(:subtree_measure_stores) => non_neg_integer(),
          required(:resolve_hits) => non_neg_integer(),
          required(:resolve_misses) => non_neg_integer(),
          required(:resolve_stores) => non_neg_integer()
        }

  @type renderer_cache_kind_stats :: %{
          required(:candidates) => non_neg_integer(),
          required(:visible_candidates) => non_neg_integer(),
          required(:suppressed_by_parent) => non_neg_integer(),
          required(:admitted) => non_neg_integer(),
          required(:hits) => non_neg_integer(),
          required(:misses) => non_neg_integer(),
          required(:stores) => non_neg_integer(),
          required(:evictions) => non_neg_integer(),
          required(:stale_evictions) => non_neg_integer(),
          required(:rejected) => non_neg_integer(),
          required(:current_entries) => non_neg_integer(),
          required(:current_bytes) => non_neg_integer(),
          required(:current_gpu_payloads) => non_neg_integer(),
          required(:current_cpu_payloads) => non_neg_integer(),
          required(:evicted_bytes) => non_neg_integer(),
          required(:stale_evicted_bytes) => non_neg_integer(),
          required(:gpu_payload_stores) => non_neg_integer(),
          required(:cpu_payload_stores) => non_neg_integer(),
          required(:prepare_successes) => non_neg_integer(),
          required(:prepare_failures) => non_neg_integer(),
          required(:direct_fallbacks_after_admission) => non_neg_integer(),
          required(:rejected_ineligible) => non_neg_integer(),
          required(:rejected_admission) => non_neg_integer(),
          required(:rejected_oversized) => non_neg_integer(),
          required(:rejected_payload_budget) => non_neg_integer(),
          required(:rejected_fractional_placement) => non_neg_integer(),
          required(:rejected_unsupported_transform) => non_neg_integer(),
          required(:prepare) => duration_stats(),
          required(:draw_hit) => duration_stats()
        }

  @type renderer_cache_stats :: %{
          required(:enabled) => boolean(),
          required(:disabled_reason) => String.t() | nil,
          required(:paint_layer) => renderer_cache_kind_stats()
        }

  @type rendering_api_info :: %{
          required(:requested) => atom() | String.t(),
          required(:selected) => atom() | String.t()
        }

  @typedoc """
  Native stats payload. Current schema version: 25.
  """
  @type stats_snapshot :: %{
          required(:version) => pos_integer(),
          required(:kind) => String.t(),
          required(:enabled) => boolean(),
          required(:rendering_api) => rendering_api_info() | nil,
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
            required(:render_draw) => duration_stats(),
            required(:render_flush) => duration_stats(),
            required(:render_gpu_flush) => duration_stats(),
            required(:render_submit) => duration_stats(),
            required(:present_submit) => duration_stats(),
            required(:pipeline) => duration_stats(),
            required(:pipeline_submit_to_tree_start) => duration_stats(),
            required(:pipeline_tree) => duration_stats(),
            required(:pipeline_render_queue) => duration_stats(),
            required(:pipeline_submit_to_swap) => duration_stats(),
            required(:pipeline_swap_to_frame_callback) => duration_stats(),
            required(:layout) => duration_stats(),
            required(:refresh) => duration_stats(),
            required(:event_resolve) => duration_stats(),
            required(:patch_tree_process) => duration_stats()
          },
          required(:drm) => %{
            required(:forced_gpu_finish_before_swap) => duration_stats(),
            required(:forced_gpu_finish_after_swap) => duration_stats(),
            required(:gpu_render_elapsed) => duration_stats(),
            required(:gpu_render_elapsed_disjoint_discarded_samples) => non_neg_integer(),
            required(:gpu_render_elapsed_pool_saturated_sample_skips) => non_neg_integer(),
            required(:gpu_render_elapsed_stale_epoch_samples) => non_neg_integer(),
            required(:video_retired_gl_finish_fallbacks) => non_neg_integer(),
            required(:egl_swap_buffers) => duration_stats(),
            required(:gbm_lock_front_buffer) => duration_stats(),
            required(:framebuffer_lookup) => duration_stats(),
            required(:prepared_to_commit) => duration_stats(),
            required(:previous_flip_to_commit) => duration_stats(),
            required(:atomic_commit_ioctl) => duration_stats(),
            required(:commit_to_kernel_page_flip) => duration_stats(),
            required(:kernel_page_flip_interval) => duration_stats(),
            required(:page_flip_dispatch_delay) => duration_stats(),
            required(:commit_to_event_processed) => duration_stats(),
            required(:page_flip_events) => non_neg_integer(),
            required(:page_flip_sequence_steps) => non_neg_integer(),
            required(:missed_vblanks) => non_neg_integer()
          },
          required(:counters) => %{
            required(:pipeline) => %{
              required(:scenes_constructed) => non_neg_integer(),
              required(:render_queue_overwrites) => non_neg_integer(),
              required(:drm_scenes_selected_for_draw) => non_neg_integer(),
              required(:drm_scenes_presented) => non_neg_integer()
            },
            required(:video) => %{
              required(:submitted) => non_neg_integer(),
              required(:inactive_dropped) => non_neg_integer(),
              required(:pending_replaced) => non_neg_integer(),
              required(:pending_taken) => non_neg_integer(),
              required(:imported) => non_neg_integer(),
              required(:leases_released) => non_neg_integer(),
              required(:retired_fences_created) => non_neg_integer(),
              required(:retired_fences_released) => non_neg_integer(),
              required(:retired_gl_finish_fallbacks) => non_neg_integer(),
              required(:acquire_fences_received) => non_neg_integer(),
              required(:acquire_server_waits_queued) => non_neg_integer(),
              required(:acquire_client_wait_fallbacks) => non_neg_integer(),
              required(:acquire_wait_timeouts) => non_neg_integer(),
              required(:acquire_wait_errors) => non_neg_integer(),
              required(:primary_prepared) => non_neg_integer(),
              required(:video_primary_prepared) => non_neg_integer(),
              required(:stale_prepared) => non_neg_integer(),
              required(:stale_video_prepared) => non_neg_integer(),
              required(:gbm_no_free) => non_neg_integer(),
              required(:primary_commit_attempts) => non_neg_integer(),
              required(:primary_commit_ebusy) => non_neg_integer(),
              required(:primary_committed) => non_neg_integer(),
              required(:primary_presented) => non_neg_integer(),
              required(:video_primary_presented) => non_neg_integer(),
              required(:video_primary_ever_presented) => boolean(),
              required(:last_presented_streams) => [
                %{
                  required(:renderer_epoch) => non_neg_integer(),
                  required(:target_id) => binary(),
                  required(:target_incarnation) => non_neg_integer(),
                  required(:stream_id) => non_neg_integer()
                }
              ],
              required(:page_flip_events) => non_neg_integer(),
              required(:page_flip_sequence_steps) => non_neg_integer(),
              required(:missed_vblanks) => non_neg_integer(),
              required(:current_pending) => non_neg_integer(),
              required(:current_direct_imports) => non_neg_integer(),
              required(:current_retired_imports) => non_neg_integer(),
              required(:max_retired_imports) => non_neg_integer(),
              required(:current_prepared) => non_neg_integer(),
              required(:current_in_flight) => non_neg_integer()
            },
            required(:vulkan_video) => %{
              required(:acquire_sync_fd_imported) => non_neg_integer(),
              required(:temporary_semaphore_import_failures) => non_neg_integer(),
              required(:ownership_acquires_submitted) => non_neg_integer(),
              required(:acquire_submit_failures) => non_neg_integer(),
              required(:ganesh_waits_rejected) => non_neg_integer(),
              required(:releases_submitted) => non_neg_integer(),
              required(:release_submit_failures) => non_neg_integer(),
              required(:releases_completed) => non_neg_integer(),
              required(:release_fences_created) => non_neg_integer(),
              required(:release_fence_errors) => non_neg_integer(),
              required(:release_fence_completions) => non_neg_integer(),
              required(:retirement_timeouts) => non_neg_integer(),
              required(:import_cap_saturations) => non_neg_integer(),
              required(:quarantined) => non_neg_integer(),
              required(:global_quarantine_terminal) => boolean(),
              required(:device_lost) => non_neg_integer()
            },
            required(:layout_cache) => layout_cache_stats(),
            required(:renderer_cache) => renderer_cache_stats()
          }
        }

  @doc false
  @spec stats(reference(), stats_command()) :: {:ok, stats_snapshot()} | {:error, String.t()}
  def stats(_resource, _command), do: :erlang.nif_error(:nif_not_loaded)

  @type vulkan_drm_node_info :: %{
          required(:path) => String.t(),
          required(:match_field) => :primary | :render | String.t(),
          required(:major) => non_neg_integer(),
          required(:minor) => non_neg_integer()
        }

  @type vulkan_device_info :: %{
          required(:physical_device_name) => String.t(),
          required(:driver_name) => String.t() | nil,
          required(:driver_id) => atom() | String.t() | nil,
          required(:software) => boolean(),
          required(:drm_node) => vulkan_drm_node_info() | nil
        }

  @type renderer_info :: %{
          required(:backend) => atom() | String.t(),
          required(:rendering_api) => rendering_api_info(),
          required(:capabilities) => %{
            required(:gpu) => boolean(),
            required(:renderer_cache) => boolean(),
            required(:screenshot) => boolean(),
            required(:raster_present) => [atom() | String.t()],
            required(:prime_video) => boolean(),
            required(:prime_video_formats) => [String.t()]
          },
          required(:vulkan_device) => vulkan_device_info() | nil
        }

  @doc false
  @spec renderer_info(reference()) :: {:ok, renderer_info()} | {:error, String.t()}
  def renderer_info(_renderer), do: :erlang.nif_error(:nif_not_loaded)

  @doc false
  @spec renderer_capture_pixels(reference(), map()) :: {:ok, binary()} | {:error, String.t()}
  def renderer_capture_pixels(_renderer, _opts), do: :erlang.nif_error(:nif_not_loaded)

  @doc false
  @spec renderer_capture_png(reference(), map()) :: {:ok, binary()} | {:error, String.t()}
  def renderer_capture_png(_renderer, _opts), do: :erlang.nif_error(:nif_not_loaded)

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
