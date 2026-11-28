defmodule EmergeSkia.Native do
  @moduledoc """
  NIF bindings for the Skia renderer.
  """

  # Mix helpers are not normal lib modules; changes must still invalidate this NIF wrapper.
  @external_resource Path.expand("../../mix/native.exs", __DIR__)
  @external_resource Path.expand("../../mix/targets.exs", __DIR__)

  @behaviour VideoInterop.AbandonmentGuard

  @checksum_only EmergeSkia.BuildConfig.checksum_only_mode?()

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
    @load_native_runtime Application.compile_env(:emerge, :load_macos_nif, false) or
                           EmergeSkia.BuildConfig.load_native_runtime?()

    if @load_native_runtime do
      @rustler_opts Mix.Project.config()[:rustler_opts] || []
      @crate_path Path.expand("../../native/emerge_skia", __DIR__)
      @compiled_backends EmergeSkia.BuildConfig.compiled_backends()
      @compiled_vulkan_backends EmergeSkia.BuildConfig.compiled_vulkan_backends()
      @compiled_opengl_backends EmergeSkia.BuildConfig.compiled_opengl_backends()
      @checksum_path Path.expand("../../checksum-Elixir.EmergeSkia.Native.exs", __DIR__)
      @version Mix.Project.config()[:version]
      @base_url {EmergeSkia.BuildConfig, :precompiled_tar_gz_url}
      @precompiled_targets EmergeSkia.BuildConfig.precompiled_targets()
      @precompiled_nif_versions EmergeSkia.BuildConfig.precompiled_nif_versions()
      @precompiled_variants EmergeSkia.BuildConfig.precompiled_variants(
                              System.get_env(),
                              @compiled_backends,
                              @compiled_vulkan_backends,
                              @compiled_opengl_backends
                            )
      @platform_cargo_features EmergeSkia.BuildConfig.rustler_platform_features(
                                 System.get_env(),
                                 @compiled_backends,
                                 @compiled_vulkan_backends,
                                 @compiled_opengl_backends
                               )
      :ok =
        EmergeSkia.BuildProfiles.validate_target!(
          @rustler_opts[:target] || EmergeSkia.BuildProfiles.rust_target(System.get_env()),
          EmergeSkia.BuildConfig.canonical_features(
            @compiled_backends,
            @compiled_vulkan_backends,
            @compiled_opengl_backends
          )
        )

      @cargo_features EmergeSkia.BuildConfig.compiled_backends_to_rustler_features(
                        @compiled_backends,
                        @compiled_vulkan_backends,
                        @compiled_opengl_backends
                      ) ++ @platform_cargo_features
      # mix.exs already resolves the Rust target from the Nerves compiler.
      # RustlerPrecompiled reads TARGET_ARCH instead of Rustler's :target option.
      @original_target_arch System.get_env("TARGET_ARCH")

      try do
        if target = @rustler_opts[:target] do
          [arch | _] = String.split(target, "-")
          System.put_env("TARGET_ARCH", arch)
        end

        @force_build EmergeSkia.BuildConfig.force_precompiled_build?(
                       report_fallback: true,
                       checksum_path: @checksum_path,
                       compiled_backends: @compiled_backends,
                       compiled_vulkan_backends: @compiled_vulkan_backends,
                       compiled_opengl_backends: @compiled_opengl_backends,
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
      after
        if @original_target_arch do
          System.put_env("TARGET_ARCH", @original_target_arch)
        else
          System.delete_env("TARGET_ARCH")
        end
      end
    end
  end

  @doc false
  def set_window_title(_renderer, _title), do: :erlang.nif_error(:nif_not_loaded)

  @doc false
  def set_frame_rate(renderer, fps), do: control(renderer, :frame_rate, fps)

  @doc false
  def configure(renderer, config) do
    config = Map.put_new(config, :drm_power, nil)
    configure_nif(renderer, Map.update!(config, :drm_power, &power_defaults/1))
  end

  @doc false
  def power_defaults(nil), do: nil
  def power_defaults(power), do: Map.put_new(power, :wake_on_touch, nil)

  @doc false
  def configure_nif(renderer, %{title: title} = config)
      when is_nil(title) or (is_binary(title) and byte_size(title) <= 4096),
      do: control(renderer, :configure, config)

  def configure_nif(_, _), do: {:error, "invalid renderer config"}

  @doc false
  def new_backlight_state, do: :erlang.nif_error(:nif_not_loaded)

  @doc false
  def config_status(_renderer), do: :erlang.nif_error(:nif_not_loaded)

  @doc false
  def drm_outputs(_card_path), do: :erlang.nif_error(:nif_not_loaded)
  @doc false
  def renderer_status(_renderer), do: :erlang.nif_error(:nif_not_loaded)
  @doc false
  def stop_timeout(renderer, ms) when is_integer(ms) and ms >= 0 do
    with {:ok, :ok} <-
           await(
             ms,
             "renderer cleanup is still pending (timeout)",
             &renderer_stop_nif(renderer, ms, &1)
           ),
         do: :ok
  end

  @doc false
  def renderer_stop_nif(_renderer, _ms, _id), do: :erlang.nif_error(:nif_not_loaded)

  @doc """
  Private window NIF binding. Application startup uses
  `EmergeSkia.start(%Emerge.Config{})`, not this positional transport entrypoint.
  Returns a renderer resource for internal operations.
  """
  @spec start(String.t(), non_neg_integer(), non_neg_integer()) ::
          reference() | {:ok, reference()} | {:error, term()}
  def start(_title, _width, _height), do: :erlang.nif_error(:nif_not_loaded)

  @doc """
  Start through the private native map ABI.

  Internal config/option adapters derive this map from normalized config. These transport names are not accepted public startup keywords.
  """
  @spec start_opts(%{
          required(:owner) => pid() | nil,
          required(:backend) => String.t(),
          required(:rendering_api) => %{
            required(:kind) => String.t(),
            required(:raster_present) => String.t(),
            required(:raster_present_configured) => boolean()
          },
          required(:title) => String.t(),
          required(:width) => non_neg_integer(),
          required(:height) => non_neg_integer(),
          required(:always_on_top) => boolean(),
          required(:drm_card) => String.t() | nil,
          required(:drm_output) => String.t() | nil,
          required(:drm_mode) => String.t() | nil,
          required(:vulkan_drm_node) => String.t() | nil,
          required(:drm_hotplug_interval_ms) => pos_integer() | nil,
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
          required(:asset_svg_tree_max_entries) => non_neg_integer(),
          required(:asset_svg_tree_max_bytes) => non_neg_integer(),
          required(:asset_decode_at_size) => boolean(),
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
          required(:target_fps) => pos_integer() | nil,
          required(:tree_headroom_percent) => 0..1000,
          required(:headless) => %{
            required(:target) => pid() | nil,
            required(:mode) => String.t(),
            required(:pixel_format) => String.t(),
            required(:bw1_polarity) => String.t(),
            required(:dither) => boolean(),
            required(:frame_message) => String.t(),
            required(:prime) => %{
              required(:max_in_flight) => pos_integer(),
              required(:on_backpressure) => String.t()
            }
          }
        }) :: reference() | {:ok, reference()} | {:error, term()}
  def start_opts(opts) do
    observer = self()

    with true <- bounded_start?(opts) || {:error, "invalid renderer startup fields"},
         {:ok, renderer} <-
           await(6_000, "renderer startup timed out", &renderer_start_nif(opts, observer, &1)),
         do: renderer
  end

  defp bounded_start?(%{
         backend: backend,
         title: title,
         rendering_api: %{kind: api, raster_present: present},
         headless: %{mode: mode, frame_message: msg, pixel_format: format}
       }),
       do:
         bounded_binary?(title, 4096) and bounded_binary?(msg, 255) and
           Enum.all?([backend, api, present, mode, format], &bounded_binary?(&1, 32))

  defp bounded_start?(_), do: false
  defp bounded_binary?(b, n), do: is_binary(b) and byte_size(b) <= n
  defp bounded_list?([], _, _), do: true

  defp bounded_list?([b | tail], n, len) when n > 0,
    do: bounded_binary?(b, len) and bounded_list?(tail, n - 1, len)

  defp bounded_list?(_, _, _), do: false

  defp renderer_start_nif(_opts, _observer, _id), do: :erlang.nif_error(:nif_not_loaded)

  @doc """
  Stop the renderer and close the window.
  """
  @spec stop(reference()) :: {:ok, :ok} | {:error, String.t()}
  def stop(renderer) do
    with :ok <- stop_timeout(renderer, 5_000), do: {:ok, :ok}
  end

  @doc """
  Accept a full EMRG declaration for layout and rendering.
  Window dimensions come from the initial start config and resize events.
  """
  @spec renderer_upload(reference(), binary()) :: :ok | {:error, term()}
  def renderer_upload(renderer, data), do: renderer_patch(renderer, data)

  @doc """
  Accept an EMRG patch, without waiting for layout or presentation. A busy
  renderer has not accepted or queued the patch.
  Window dimensions come from the initial start config and resize events.
  """
  @spec renderer_patch(reference(), binary()) :: :ok | {:error, term()}
  def renderer_patch(renderer, bytes) when is_binary(bytes) do
    case submit_revision(renderer, nil, bytes, false, nil) do
      {:ok, _} -> :ok
      error -> error
    end
  end

  @doc false
  def submit_revision(renderer, base, <<>>, false, _waiter),
    do: renderer_revision_nif(renderer, base)

  def submit_revision(renderer, base, bytes, routes?, waiter) when is_binary(bytes) do
    with {:ok, ticket, id} <-
           renderer_begin_nif(renderer, base, byte_size(bytes), routes?, waiter) do
      try do
        with :ok <- feed(ticket, id, bytes, 0), :ok <- renderer_prove_nif(ticket) do
          receive do
            {:emerge_edit, ^id, :ready} -> apply_edit(ticket, id)
            {:emerge_edit, ^id, result} -> result
          end
        else
          {:finished, result} -> result
          error -> discard_edit(ticket, id, error)
        end
      after
        renderer_abort_nif(ticket)
      end
    end
  end

  # Split in BEAM before the private NIF inspects anything. A bit-offset
  # sub-binary may need aligning, but at most one small slice is copied.
  defp feed(_ticket, _id, <<>>, _offset), do: :ok

  defp feed(ticket, id, bytes, offset) do
    n = min(byte_size(bytes), 4096)
    <<part::binary-size(^n), rest::binary>> = bytes

    case renderer_chunk_nif(ticket, offset, part) do
      :ok -> feed(ticket, id, rest, offset + n)
      :held -> with :ok <- resume_edit(id), do: feed(ticket, id, rest, offset + n)
      error -> error
    end
  end

  # Only the opt-in stage probe can return :held. It never parks a scheduler.
  defp resume_edit(id) do
    receive do
      {:emerge_resume, ^id} -> :ok
      {:emerge_edit, ^id, result} -> {:finished, result}
    end
  end

  defp apply_edit(ticket, id) do
    case renderer_apply_nif(ticket) do
      :more ->
        apply_edit(ticket, id)

      :held ->
        case resume_edit(id) do
          :ok -> apply_edit(ticket, id)
          {:finished, result} -> result
        end

      :ok ->
        receive do
          {:emerge_edit, ^id, result} -> result
        end

      error ->
        discard_edit(ticket, id, error)
    end
  end

  defp discard_edit(ticket, id, error) do
    renderer_abort_nif(ticket)

    receive do
      {:emerge_edit, ^id, _result} -> error
    end
  end

  @doc false
  def renderer_stage_nif(_renderer, _stage, _hold), do: :erlang.nif_error(:nif_not_loaded)

  @doc false
  def renderer_storage_nif(_renderer), do: :erlang.nif_error(:nif_not_loaded)

  @doc false
  def renderer_revision_nif(_renderer, _base), do: :erlang.nif_error(:nif_not_loaded)

  defp renderer_begin_nif(_renderer, _base, _size, _routes, _waiter),
    do: :erlang.nif_error(:nif_not_loaded)

  defp renderer_chunk_nif(_ticket, _offset, _part), do: :erlang.nif_error(:nif_not_loaded)
  defp renderer_prove_nif(_ticket), do: :erlang.nif_error(:nif_not_loaded)
  defp renderer_apply_nif(_ticket), do: :erlang.nif_error(:nif_not_loaded)
  defp renderer_abort_nif(_ticket), do: :erlang.nif_error(:nif_not_loaded)

  @doc """
  Measure text dimensions.

  Returns `{width, line_height, ascent, descent}`.
  """
  @spec measure_text(String.t(), float()) :: {float(), float(), float(), float()}
  def measure_text(_text, _font_size), do: :erlang.nif_error(:nif_not_loaded)

  @doc """
  Load a font from binary data and register it with a name.

  ## Parameters
  - `renderer` - Renderer that owns the font registration
  - `name` - Font family name to register (e.g., "my-font")
  - `weight` - Font weight (100-900, 400=normal, 700=bold)
  - `italic` - Whether this is an italic variant
  - `data` - Binary font data (TTF file contents)

  ## Example
      {:ok, data} = File.read("fonts/MyFont-Bold.ttf")
      {:ok, true} = EmergeSkia.Native.load_font_nif(renderer, "my-font", 700, false, data)
  """
  @spec load_font_nif(reference(), String.t(), non_neg_integer(), boolean(), binary()) ::
          {:ok, boolean()} | {:error, String.t()}
  def load_font_nif(_renderer, _name, _weight, _italic, _data),
    do: :erlang.nif_error(:nif_not_loaded)

  @doc """
  Configure native asset loading policy and source roots.
  """
  @spec configure_assets_nif(reference(), map()) :: :ok
  def configure_assets_nif(
        renderer,
        %{sources: roots, allowlist: paths, extensions: extensions} = opts
      ) do
    if bounded_list?(roots, 1, 4096) and bounded_list?(paths, 128, 4096) and
         bounded_list?(extensions, 64, 32) do
      with {:ok, :ok} <-
             await(5_000, "asset setup timed out", &renderer_assets_nif(renderer, opts, &1)),
           do: :ok
    else
      {:error, "asset configuration exceeds storage limit"}
    end
  end

  defp renderer_assets_nif(_renderer, _opts, _id), do: :erlang.nif_error(:nif_not_loaded)

  @doc """
  Check if the renderer is still running.
  """
  @spec is_running(reference()) :: boolean()
  def is_running(_renderer), do: :erlang.nif_error(:nif_not_loaded)

  @doc false
  @spec video_frame_submit(reference(), String.t(), VideoInterop.Frame.t()) ::
          {:ok, :transferred | :released}
          | {:error, {:caller_owned | :transferred, String.t()}}
  def video_frame_submit(_renderer, _target, _frame), do: :erlang.nif_error(:nif_not_loaded)

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

  The tree is an encoded EMRG binary. Offline tree-rendering helpers build the
  private map; it is not the public renderer-start config.
  It shares asset policy with `Emerge.Config.Assets` and adds offscreen asset mode.
  """
  @spec render_tree_to_pixels_nif(
          binary(),
          map()
        ) :: binary() | {:ok, binary()} | {:error, String.t()}
  def render_tree_to_pixels_nif(_data, _opts),
    do: :erlang.nif_error(:nif_not_loaded)

  @doc """
  Render a tree to an encoded PNG binary (synchronous, no window).

  The tree is an encoded EMRG binary. Offline tree-rendering helpers build the
  private map; it is not the public renderer-start config.
  It shares asset policy with `Emerge.Config.Assets` and adds offscreen asset mode.
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
  Private observer-mask transport; applications declare `config.input.events`.
  This filters raw forwarding only, not element interactions or window close.

  Mask bits:
  - 0x01: Key events
  - 0x02: Text commit/preedit/clear/surrounding deletion events
  - 0x04: Cursor position events
  - 0x08: Cursor button events
  - 0x10: Cursor scroll events
  - 0x20: Cursor enter/exit events
  - 0x40: Resize events
  - 0x80: Focus events
  - 0x100: Direct touch contacts
  - 0x1FF: All input events
  """
  @spec set_input_mask(reference(), non_neg_integer()) :: :ok
  def set_input_mask(renderer, mask), do: control(renderer, :mask, mask)

  @doc """
  Set the target process to receive input events.

  Input events are sent directly to the target process as
  `{:emerge_skia_event, event}` messages.
  """
  @spec set_input_target(reference(), pid() | nil) :: :ok
  def set_input_target(renderer, pid), do: control(renderer, :input_target, pid)

  @doc false
  def send_input(renderer, event) do
    if bounded_input?(event),
      do: control(renderer, :input, event),
      else: {:error, "invalid or unsupported raw input"}
  end

  # Preflight the envelope before copying it to the disposable reply process.
  # Native still validates button/key names, actions and finite coordinates.
  defp bounded_input?({:pointer_button, {btn, act, mods, xy}}),
    do: is_atom(btn) and is_atom(act) and point?(xy) and modifiers?(mods, 4)

  defp bounded_input?({:key, {key, act, mods}}),
    do: is_atom(key) and is_atom(act) and modifiers?(mods, 4)

  defp bounded_input?({:text_commit, {text, mods}}),
    do: bounded_binary?(text, 65_536) and modifiers?(mods, 4)

  defp bounded_input?({:touch, {id, phase, xy}}),
    do:
      is_integer(id) and id in 0..0xFFFF_FFFF_FFFF_FFFF and phase in [:down, :move, :up, :cancel] and
        point?(xy)

  defp bounded_input?({:pointer_move, xy}), do: point?(xy)
  defp bounded_input?({:pointer_scroll, {delta, xy}}), do: point?(delta) and point?(xy)
  defp bounded_input?({tag, b}) when tag in [:pointer_enter, :focus], do: is_boolean(b)
  defp bounded_input?(_), do: false

  defp point?({x, y}), do: coord?(x) and coord?(y)
  defp point?(_), do: false
  defp coord?(n), do: is_float(n) or (is_integer(n) and n >= -3.403e38 and n <= 3.403e38)
  defp modifiers?([], _), do: true
  defp modifiers?([m | rest], n) when n > 0 and is_atom(m), do: modifiers?(rest, n - 1)
  defp modifiers?(_, _), do: false

  @doc """
  Set the target process to receive native renderer log messages.

  Native logs are sent directly to the target process as
  `{:emerge_skia_log, level, source, message}` messages.
  """
  @spec set_log_target(reference(), pid() | nil) :: :ok
  def set_log_target(renderer, pid), do: control(renderer, :log_target, pid)

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
  def stats(renderer, command), do: control(renderer, :stats, command)

  defp control(renderer, op, payload) do
    observer = self()

    await(
      5000,
      "renderer control timed out",
      &renderer_control_nif(renderer, op, payload, observer, &1)
    )
  end

  defp renderer_control_nif(_renderer, _op, _payload, _observer, _id),
    do: :erlang.nif_error(:nif_not_loaded)

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
  def renderer_capture_pixels(renderer, opts),
    do: capture(renderer, opts, false)

  @doc false
  @spec renderer_capture_png(reference(), map()) :: {:ok, binary()} | {:error, String.t()}
  def renderer_capture_png(renderer, opts),
    do: capture(renderer, opts, true)

  defp capture(renderer, opts, png?) do
    if Enum.all?(
         [opts.pixel_format, opts.background, opts.png_compression],
         &bounded_binary?(&1, 32)
       ),
       do:
         await(
           opts.timeout_ms,
           "screenshot timed out",
           &renderer_capture_nif(renderer, opts, png?, &1)
         ),
       else: {:error, "unsupported screenshot options"}
  end

  defp renderer_capture_nif(_renderer, _opts, _png, _id), do: :erlang.nif_error(:nif_not_loaded)

  # A disposable return address keeps late binaries and session handles out of
  # the caller's mailbox. There is no waiting (even on a dirty scheduler) in
  # the native entry. The request itself is admitted before starting its timer.
  defp await(ms, timeout, request) do
    caller = self()
    reply = :erlang.alias([:reply])

    {pid, ref} =
      spawn_monitor(fn ->
        parent = Process.monitor(caller)
        id = :erlang.unique_integer([:positive])

        result =
          case request.(id) do
            :complete ->
              {:ok, :ok}

            :ok ->
              receive do
                {:emerge_skia_reply, ^id, result} -> result
                {:DOWN, ^parent, :process, ^caller, _} -> exit(:normal)
              after
                ms -> {:error, timeout}
              end

            error ->
              error
          end

        send(reply, {reply, result})
      end)

    try do
      receive do
        {^reply, result} -> result
        {:DOWN, ^ref, :process, ^pid, reason} -> {:error, reason}
      after
        ms + 1_000 ->
          Process.exit(pid, :kill)
          {:error, timeout}
      end
    after
      :erlang.unalias(reply)
      Process.demonitor(ref, [:flush])
    end
  end

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
end
