defmodule EmergeSkia do
  @moduledoc """
  Low-level renderer API used by Emerge viewports.

  Most applications should define a viewport with `use Emerge`. Use
  `EmergeSkia` directly when integrating the renderer without the viewport
  process or when using renderer-specific APIs such as capture, statistics, or
  video targets.

  ## Example

      {:ok, renderer} =
        EmergeSkia.start(%Emerge.Config{
          output: %Emerge.Config.Window{title: "My App"},
          assets: [otp_app: :my_app]
        })

      {_state, _assigned_tree} = EmergeSkia.upload_tree(renderer, tree)
      :ok = EmergeSkia.stop(renderer)

  `start/1` accepts only `Emerge.Config`; see the
  [configuration reference](viewport-configuration.html) and
  [0.5 migration guide](0-5.html). `renderer_info/1` reports the
  backend and rendering API actually selected. `stats/2` and the capture
  functions provide on-demand diagnostics for a running renderer.

  For UI colors, prefer `Emerge.UI.Color`. `rgb/3` and `rgba/4` return packed
  unsigned `0xRRGGBBAA` values for low-level interfaces.

  When an input target is registered with `set_input_target/2`, Wayland close
  requests arrive as `{:emerge_skia_close, :window_close_requested}` independently
  of `config.input.events`. Native payload names/actions differ from the typed
  `Emerge.Input` values passed to a viewport's `handle_input/2`.
  """

  alias EmergeSkia.Assets
  alias EmergeSkia.HeadlessPrimeSession
  alias EmergeSkia.Macos.Renderer
  alias EmergeSkia.Native
  alias EmergeSkia.Options
  alias EmergeSkia.Transport
  alias EmergeSkia.TreeRenderer

  @typedoc "Renderer handle returned by `start/1`."
  @type renderer :: reference() | struct()
  @type color :: non_neg_integer()

  @type drm_mode :: %{
          id: String.t(),
          name: String.t(),
          width: pos_integer(),
          height: pos_integer(),
          refresh_hz: float(),
          preferred: boolean(),
          interlaced: boolean()
        }
  @type drm_output :: %{
          name: String.t(),
          connector_id: non_neg_integer(),
          connected: boolean(),
          modes: [drm_mode()]
        }

  @doc """
  Lists DRM connectors and their advertised modes without changing display configuration.

  Accepts `drm_card: "/dev/dri/card0"` (the default), including stable
  `/dev/dri/by-path/...-card` paths. Returns disconnected outputs too, with
  `connected: false`. Requires Linux, compiled DRM support and read access to the card.

  Pass an output's `name` as `Config.DRM.connector` and a mode map (or its
  exact `id`) as `Config.DRM.mode`. IDs describe exact timings,
  not indices or rounded refresh rates. Discovery is a snapshot; startup checks
  availability again. Explicit selections never fall back to another output/mode.

      {:ok, outputs} = EmergeSkia.drm_outputs(drm_card: "/dev/dri/card0")
      output = Enum.find(outputs, &(&1.connected and &1.name == "HDMI-A-1"))
      mode = Enum.find(output.modes, & &1.preferred)
      %Emerge.Config{output: %Emerge.Config.DRM{connector: output.name, mode: mode}}
  """
  @spec drm_outputs(keyword()) :: {:ok, [drm_output()]} | {:error, String.t()}
  def drm_outputs(opts \\ []) do
    opts |> Options.drm_discovery_card!() |> Native.drm_outputs()
  end

  @doc """
  Starts a renderer using `%Emerge.Config{}`.

  Outside a viewport, `assets.otp_app` is required to resolve logical assets.
  See `Emerge.Config` and its nested records for defaults and types. Legacy
  keyword configuration and positional startup arities have been removed.

      output = %Emerge.Config.Headless{target: self(), size: {640, 480}}
      config = %Emerge.Config{output: output, renderer: [api: :raster], assets: [otp_app: :my_app]}
      {:ok, renderer} = EmergeSkia.start(config)

  Native logs go to the starting process as
  `{:emerge_skia_log, level, source, message}`. Use `set_log_target/2` to change
  the destination. Unsupported controls raise before native startup.
  """

  @spec start(Emerge.Config.t()) :: {:ok, renderer()} | {:error, term()}
  def start(%Emerge.Config{} = config), do: start_owned(config, nil)

  def start(_other) do
    raise ArgumentError,
          "EmergeSkia.start/1 expects %Emerge.Config{}; keyword configuration was removed in 0.5"
  end

  @doc false
  def start_owned(%Emerge.Config{} = config, owner, backlight_state \\ nil) do
    config = Emerge.Config.Normalize.config!(config)
    validate_config!(config)

    if is_nil(config.assets.otp_app),
      do: raise(ArgumentError, "config.assets.otp_app is required outside a viewport")

    backlight_state =
      if match?(%Emerge.Config.DRM{}, config.output),
        do: backlight_state || Native.new_backlight_state(),
        else: nil

    opts =
      config
      |> Emerge.Config.Native.options()
      |> Keyword.put(:owner, owner)
      |> Keyword.put(:backlight_state, backlight_state)

    asset_config = Assets.normalize_asset_config!(opts)

    native_opts =
      opts
      |> Options.build_start_native_opts!()
      |> Map.merge(Assets.native_start_asset_config(asset_config))
      |> Map.put(:drm_cursor, Assets.normalize_drm_cursor_overrides!(opts))

    if native_opts.drm_cursor != [] and String.downcase(native_opts.backend) == "wayland" do
      raise ArgumentError, "drm_cursor is only supported with backend: :drm"
    end

    case Options.rendering_api_start_error(native_opts) do
      nil ->
        transport = Transport.for_backend(native_opts.backend)

        with {:ok, renderer} <- transport.start_session(native_opts, asset_config) do
          :ok = transport.set_input_mask(renderer, Emerge.Config.Native.input_mask(config.input))
          {:ok, renderer}
        end

      reason ->
        {:error, {:error, reason}}
    end
  end

  # Explicit OpenGL and CPU raster support safe stop/recreate lifecycles.
  # Vulkan and raster GPU-upload power controls remain unsupported.
  @doc false
  def validate_config!(%Emerge.Config{} = config) do
    case config.output do
      %Emerge.Config.DRM{display: display, backlight: backlight}
      when display != :on or not is_nil(backlight.brightness) or is_binary(backlight.device) ->
        unless supports_drm_power?(config.renderer) do
          raise ArgumentError,
                "DRM display/backlight control requires OpenGL or CPU raster presentation"
        end

      _ ->
        :ok
    end

    if not is_nil(config.renderer.target_fps) and not supports_frame_cap?(config.output) do
      raise ArgumentError,
            "target_fps is currently supported only by headless and Wayland outputs"
    end

    :ok
  end

  defp supports_drm_power?(%Emerge.Config.Renderer{api: :opengl}), do: true

  defp supports_drm_power?(%Emerge.Config.Renderer{api: :raster, raster_present: present}),
    do: present in [:auto, :cpu]

  defp supports_drm_power?(_renderer), do: false

  defp supports_frame_cap?(%Emerge.Config.Headless{}), do: true

  defp supports_frame_cap?(%Emerge.Config.Window{backend: :auto}),
    do: supports_window_frame_cap?(EmergeSkia.BuildConfig.default_runtime_backend())

  defp supports_frame_cap?(%Emerge.Config.Window{backend: backend}),
    do: supports_window_frame_cap?(backend)

  defp supports_frame_cap?(_output), do: false

  defp supports_window_frame_cap?(:wayland), do: true
  defp supports_window_frame_cap?(_backend), do: false

  @typedoc "Session-scoped native live-control submission token."
  @type config_token :: {non_neg_integer(), pos_integer()}

  @typedoc "Native live-control progress, distinct from rendering or presentation."
  @type config_control_status :: %{
          session_id: non_neg_integer(),
          desired_revision: non_neg_integer(),
          pending_revision: pos_integer() | nil,
          applied_revision: pos_integer() | nil,
          failed_revision: pos_integer() | nil,
          error: String.t() | nil
        }

  @doc false
  @spec configure(renderer(), Emerge.Config.normalized_t(), Emerge.Config.normalized_t()) ::
          :ok | {:ok, config_token()} | {:error, term()}
  def configure(%Renderer{} = renderer, previous, config) do
    with :ok <- validate_config_change(previous, config),
         :ok <- configure_title(renderer, config.output) do
      Transport.for_renderer(renderer).set_input_mask(
        renderer,
        Emerge.Config.Native.input_mask(config.input)
      )
    end
  end

  def configure(renderer, previous, config) do
    with :ok <- validate_config_change(previous, config) do
      title =
        case config.output do
          %Emerge.Config.Window{title: title} -> title
          _ -> nil
        end

      Native.configure(Transport.Native.native_renderer(renderer), %{
        title: title,
        target_fps: config.renderer.target_fps,
        input_mask: Emerge.Config.Native.input_mask(config.input),
        drm_power:
          case config.output do
            %Emerge.Config.DRM{} = output ->
              %{display_on: output.display == :on, brightness: output.backlight.brightness}

            _ ->
              nil
          end
      })
    end
  end

  @doc """
  Reports native live-control revisions for this renderer session.

  `desired_revision` is the latest accepted submission, `pending_revision` awaits
  owner application, and `applied_revision` is the last confirmed live-control
  revision. Failures include `failed_revision` and `error`; a failed update may
  have partial effects, so the last applied revision is not a rollback guarantee.
  Revision zero means no live submission (startup settings are not counted). This
  reports native owner processing, not independent verification of physical display
  state. macOS host revision reporting is not implemented.

  Native `configure/3` submissions send `{:emerge_skia_config, {session_id, revision},
  :ok | {:error, reason}}` to the submitting process. Session shutdown can end
  delivery; use renderer status/recovery as the terminal ownership authority.
  """
  @spec config_status(renderer()) :: {:ok, config_control_status()} | {:error, :unsupported}
  def config_status(%Renderer{}), do: {:error, :unsupported}

  def config_status(renderer),
    do: {:ok, renderer |> Transport.Native.native_renderer() |> Native.config_status()}

  @doc false
  @spec validate_config_change(Emerge.Config.normalized_t(), Emerge.Config.normalized_t()) ::
          :ok | {:error, {:unsupported_live_config, :target_fps | :drm_output}}
  def validate_config_change(previous, config) do
    Emerge.Config.Changes.validate!(previous, config)

    cond do
      previous.renderer.target_fps != config.renderer.target_fps and
          not supports_frame_cap?(config.output) ->
        {:error, {:unsupported_live_config, :target_fps}}

      match?(%Emerge.Config.DRM{}, config.output) and
          (previous.output.connector != config.output.connector or
             previous.output.mode != config.output.mode or
             (previous.output != config.output and not supports_drm_power?(config.renderer))) ->
        {:error, {:unsupported_live_config, :drm_output}}

      true ->
        :ok
    end
  end

  defp configure_title(%Renderer{} = renderer, %Emerge.Config.Window{title: title}),
    do: EmergeSkia.Macos.Host.set_window_title(renderer, title)

  defp configure_title(_renderer, _output), do: :ok

  @doc """
  Stop the renderer and close its window or output session.

  Most sessions return `:ok`. Native sessions, including headless PRIME, can
  return `{:error, reason}` when ownership-safe shutdown cannot finish. Do not
  treat an error as successful cleanup. Use `renderer_status/1` to check native
  cleanup completion; a timeout does not make a DRM output reusable.
  Quarantined sessions require a BEAM process restart.
  """
  @spec stop(renderer()) :: :ok | {:error, term()}
  def stop(renderer) do
    renderer
    |> Transport.for_renderer()
    |> apply(:stop_session, [renderer])
  end

  @doc """
  Requests native shutdown and waits at most `timeout` milliseconds (default 5,000).

  A timeout does not release ownership or make an output reusable. Native cleanup
  continues; `renderer_status/1` distinguishes pending cleanup from completion.
  Quarantined sessions require a VM restart. macOS currently supports only `stop/1`.
  """
  @spec stop(renderer(), keyword()) :: :ok | {:error, term()}
  def stop(renderer, opts) when is_list(opts) do
    timeout = Keyword.get(opts, :timeout, 5_000)

    unless is_integer(timeout) and timeout >= 0 and timeout <= 4_294_967_295 do
      raise ArgumentError, "stop timeout must be a non-negative 32-bit integer"
    end

    case renderer do
      %Renderer{} ->
        {:error, :unsupported}

      %HeadlessPrimeSession{} ->
        HeadlessPrimeSession.stop(renderer, timeout)

      _ ->
        Native.stop_timeout(renderer, timeout)
    end
  end

  @doc """
  Returns native session health and cleanup status.

  `cleanup_complete` is true only after safe teardown. `running?/1 == false`
  alone does not mean a DRM output is available for a replacement renderer.
  """
  @spec renderer_status(renderer()) :: {:ok, map()} | {:error, term()}
  def renderer_status(%Renderer{}), do: {:error, :unsupported}

  def renderer_status(%HeadlessPrimeSession{} = renderer),
    do: HeadlessPrimeSession.status(renderer)

  def renderer_status(renderer) do
    case Native.renderer_status(renderer) do
      {:ok, status} -> {:ok, normalize_renderer_status(status)}
      error -> error
    end
  end

  @doc false
  def normalize_renderer_status(status) do
    states = %{
      "running" => :running,
      "stopping" => :stopping,
      "stopped" => :stopped,
      "quarantined" => :quarantined
    }

    scopes = %{"session" => :session, "output" => :output, "card" => :card, "process" => :process}
    recoveries = %{"after_cleanup" => :after_cleanup, "vm_restart" => :vm_restart}

    failure =
      case status.failure do
        nil ->
          nil

        failure ->
          %{
            failure
            | scope: Map.fetch!(scopes, failure.scope),
              recovery: Map.fetch!(recoveries, failure.recovery)
          }
      end

    %{status | state: Map.fetch!(states, status.state), failure: failure}
  end

  @doc """
  Check if the renderer window is still open.
  """
  @spec running?(renderer()) :: boolean()
  def running?(renderer) do
    renderer
    |> Transport.for_renderer()
    |> apply(:session_running?, [renderer])
  end

  @doc """
  Submits one VideoInterop frame to a renderer-local video target.

  The target is the atom passed to `Emerge.UI.video/2` in the renderer's tree.
  Hidden targets consume and drop frames immediately. Visible targets retain
  only the latest submitted frame.

  This low-level function exposes ownership on errors:

  - `:ok` means the frame was consumed.
  - `{:error, {:caller_owned, reason}}` means submission did not consume the
    frame; the caller must release borrowed storage.
  - `{:error, {:transferred, reason}}` means ownership transferred before the
    failure and the caller must not release the frame.

  Applications submitting through a viewport should normally use
  `Emerge.submit_video_frame/3`, which consumes the frame on every normal
  return and hides these ownership receipts.
  """
  @spec submit_video_frame(renderer(), atom(), VideoInterop.Frame.t()) ::
          :ok | {:error, {:caller_owned | :transferred, term()}}
  def submit_video_frame(%Renderer{} = renderer, target, %VideoInterop.Frame{} = frame) do
    with {:ok, payload} <- EmergeSkia.Macos.Video.encode(target, frame),
         :ok <- EmergeSkia.Macos.Host.submit_video(renderer, payload) do
      :ok
    else
      {:error, reason} -> {:error, {:caller_owned, reason}}
    end
  end

  def submit_video_frame(
        %HeadlessPrimeSession{} = renderer,
        target,
        %VideoInterop.Frame{} = frame
      )
      when is_atom(target),
      do: submit_native_video_frame(renderer, target, frame)

  def submit_video_frame(renderer, target, %VideoInterop.Frame{} = frame)
      when is_reference(renderer) and is_atom(target),
      do: submit_native_video_frame(renderer, target, frame)

  def submit_video_frame(_renderer, _target, %VideoInterop.Frame{}),
    do: {:error, {:caller_owned, :video_submission_unsupported}}

  defp submit_native_video_frame(renderer, target, frame) do
    renderer = EmergeSkia.Transport.Native.native_renderer(renderer)

    case Native.video_frame_submit(renderer, Atom.to_string(target), frame) do
      {:ok, _receipt} ->
        :ok

      {:error, {receipt, reason}} when receipt in [:caller_owned, :transferred] ->
        {:error, {receipt, reason}}

      {:error, reason} ->
        {:error, {:caller_owned, reason}}
    end
  end

  @doc """
  Upload a full EMRG tree, run layout, and render.

  Window dimensions come from the initial start config and are updated
  automatically when the window is resized (handled on the Rust side).
  """
  @spec upload_tree(renderer(), Emerge.tree()) ::
          {Emerge.Engine.diff_state(), Emerge.tree()}
  def upload_tree(renderer, tree) do
    TreeRenderer.upload_tree(renderer, tree)
  end

  @doc """
  Apply patches for a new tree, run layout, and render.

  Window dimensions come from the initial start config and are updated
  automatically when the window is resized (handled on the Rust side).
  """
  @spec patch_tree(renderer(), Emerge.Engine.diff_state(), Emerge.tree()) ::
          {Emerge.Engine.diff_state(), Emerge.tree()}
  def patch_tree(renderer, state, tree) do
    TreeRenderer.patch_tree(renderer, state, tree)
  end

  @doc """
  Measure text dimensions for layout purposes.

  Returns `{width, line_height, ascent, descent}` where:
  - `width` - Horizontal extent of the text
  - `line_height` - Total line height (ascent + descent)
  - `ascent` - Distance from baseline to top (positive)
  - `descent` - Distance from baseline to bottom (positive)
  """
  @spec measure_text(String.t(), number()) :: {float(), float(), float(), float()}
  def measure_text(text, font_size) do
    Transport.default().measure_text(text, font_size / 1.0)
  end

  # ===========================================================================
  # Font Loading
  # ===========================================================================

  @doc """
  Load a font from a file path.

  The font is registered by name and can be used with `Font.family/1` in elements.
  Load different variants (bold, italic) with separate calls using appropriate weight/italic params.

  The registration belongs only to `renderer` and does not change any other
  renderer or offscreen render.

  ## Parameters
  - `renderer` - Renderer returned by `start/1`
  - `name` - Font family name to register (e.g., "my-font")
  - `weight` - Font weight (100-900, 400=normal, 700=bold)
  - `italic` - Whether this is an italic variant
  - `path` - Path to the TTF font file

  ## Example

      # Load font variants into one renderer
      :ok = EmergeSkia.load_font_file(renderer, "my-font", 400, false, "priv/fonts/MyFont-Regular.ttf")
      :ok = EmergeSkia.load_font_file(renderer, "my-font", 700, false, "priv/fonts/MyFont-Bold.ttf")
      :ok = EmergeSkia.load_font_file(renderer, "my-font", 400, true, "priv/fonts/MyFont-Italic.ttf")

      # Use in elements
      el([Font.family("my-font"), Font.size(16)], text("Hello"))
      el([Font.family("my-font"), Font.bold()], text("Bold text"))
  """
  @spec load_font_file(renderer(), String.t(), non_neg_integer(), boolean(), Path.t()) ::
          :ok | {:error, term()}
  def load_font_file(renderer, name, weight, italic, path) do
    Assets.load_font_file(renderer, name, weight, italic, path)
  end

  # ===========================================================================
  # Screenshot capture
  # ===========================================================================

  @doc """
  Return pixels from the renderer's latest already-presented frame.

  This API captures retained renderer state. It no longer accepts an Emerge tree;
  pass a renderer handle returned by `start/1`.

      {:ok, pixels} =
        EmergeSkia.render_to_pixels(renderer,
          region: {0, 0, 320, 240},
          pixel_format: :rgba8888,
          timeout: 5_000
        )

  Supported options are `region: {x, y, width, height}`,
  `pixel_format: :rgba8888 | :rgb888`, `timeout`, `scale: 1.0`, and
  `background: :transparent`. macOS and headless PRIME do not support capture.
  """
  @spec render_to_pixels(renderer(), keyword()) :: {:ok, binary()} | {:error, term()}
  def render_to_pixels(renderer, opts \\ [])

  def render_to_pixels(%Renderer{} = renderer, opts) when is_list(opts) do
    capture_pixels(renderer, opts)
  end

  def render_to_pixels(%HeadlessPrimeSession{} = renderer, opts) when is_list(opts) do
    capture_pixels(renderer, opts)
  end

  def render_to_pixels(renderer, opts) when is_reference(renderer) and is_list(opts) do
    capture_pixels(renderer, opts)
  end

  def render_to_pixels(_tree, opts) when is_list(opts) do
    raise ArgumentError,
          "EmergeSkia.render_to_pixels/2 now expects a renderer handle; one-shot tree rendering was removed. Start a renderer, upload the tree, then call EmergeSkia.render_to_pixels(renderer, opts)."
  end

  @doc """
  Return an encoded PNG from the renderer's latest already-presented frame.

  This API captures retained renderer state. It no longer accepts an Emerge tree;
  pass a renderer handle returned by `start/1`.

      {:ok, png} = EmergeSkia.render_to_png(renderer, png: [compression: :default])
      File.write!("capture.png", png)

  PNG capture accepts the same region, timeout, scale, and background options
  as `render_to_pixels/2`, plus `png: [compression: :default]`. macOS and
  headless PRIME do not support capture.
  """
  @spec render_to_png(renderer(), keyword()) :: {:ok, binary()} | {:error, term()}
  def render_to_png(renderer, opts \\ [])

  def render_to_png(%Renderer{} = renderer, opts) when is_list(opts) do
    capture_png(renderer, opts)
  end

  def render_to_png(%HeadlessPrimeSession{} = renderer, opts) when is_list(opts) do
    capture_png(renderer, opts)
  end

  def render_to_png(renderer, opts) when is_reference(renderer) and is_list(opts) do
    capture_png(renderer, opts)
  end

  def render_to_png(_tree, opts) when is_list(opts) do
    raise ArgumentError,
          "EmergeSkia.render_to_png/2 now expects a renderer handle; one-shot tree rendering was removed. Start a renderer, upload the tree, then call EmergeSkia.render_to_png(renderer, opts)."
  end

  defp capture_pixels(renderer, opts) do
    opts = Options.normalize_screenshot_opts!(opts)

    renderer
    |> Transport.for_renderer()
    |> apply(:capture_pixels, [renderer, opts])
  end

  defp capture_png(renderer, opts) do
    opts = Options.normalize_screenshot_opts!(opts)

    renderer
    |> Transport.for_renderer()
    |> apply(:capture_png, [renderer, opts])
  end

  @doc """
  Convert RGB values to a color integer.

  ## Examples

      iex> EmergeSkia.rgb(255, 0, 0)
      0xFF0000FF

      iex> EmergeSkia.rgb(0, 255, 0)
      0x00FF00FF
  """
  @spec rgb(0..255, 0..255, 0..255) :: color()
  def rgb(r, g, b) do
    rgba(r, g, b, 255)
  end

  @doc """
  Convert RGBA values to a color integer.

  ## Examples

      iex> EmergeSkia.rgba(255, 0, 0, 128)
      0xFF000080

      iex> EmergeSkia.rgba(0, 0, 0, 255)
      0x000000FF
  """
  @spec rgba(0..255, 0..255, 0..255, 0..255) :: color()
  def rgba(r, g, b, a) do
    import Bitwise
    r <<< 24 ||| g <<< 16 ||| b <<< 8 ||| a
  end

  # ===========================================================================
  # Input Handling
  # ===========================================================================

  @doc """
  Set the target process to receive renderer events.

  Events are sent directly to the target process as
  `{:emerge_skia_event, event}` messages. Subscribe with `config.input.events` at
  startup; the default empty list forwards no raw input. Element events and close
  notifications remain independent.

  This is the **low-level native payload contract**, not the typed first argument
  to `c:Emerge.handle_input/2`. A viewport translates names/actions through
  `Emerge.Input`; do not match its callbacks against this table.

  Raw input event payloads include:

  - `{:cursor_pos, {x, y}}`
  - `{:cursor_button, {button, action, mods, {x, y}}}`
  - `{:cursor_scroll, {{dx, dy}, {x, y}}}`
  - `{:key, {key, action, mods}}`
  - `{:text_commit, {text, mods}}`
  - `{:text_preedit, {text, cursor}}`
  - `:text_preedit_clear`
  - `{:delete_surrounding, {before_bytes, after_bytes}}` (where the backend supports it)
  - `{:cursor_entered, entered}`
  - `{:resized, {width, height, scale}}`
  - `{:focused, focused}`

  On Wayland, close notifications are sent separately as:

  - `{:emerge_skia_close, :window_close_requested}`

  This lifecycle message bypasses the input mask so close requests are still
  delivered when other raw input categories are disabled.

  On DRM, raw `{:cursor_pos, {x, y}}` delivery is latest-wins under load so
  pointer motion does not stall rendering. Button, scroll, key, and text events
  remain ordered.

  Element event payloads include:

  - `{id_bin, event_type}`
  - `{id_bin, event_type, payload}`

  where `id_bin` is an opaque element id and `event_type` is an atom such as
  `:press`, `:click`, `:swipe_up`, `:swipe_down`, `:swipe_left`,
  `:swipe_right`, `:change`, `:key_down`, `:key_up`, or `:key_press`.

  Text-input `:change` payloads are binaries. Slider `:change` payloads are
  floats.

  Routed `:key_down`, `:key_up`, and `:key_press` payloads currently carry an
  opaque binding route id used by higher-level runtimes.

  Higher-level runtimes should route element events with
  `Emerge.Engine.lookup_event/3` or `Emerge.Engine.dispatch_event/3`/`4`.

  Where:
  - `button` is an atom like `:left`, `:right`, `:middle`
  - `action` is 0 for release, 1 for press
  - `mods` is a list of modifier atoms like `[:shift, :ctrl]`
  - `key` is a canonical atom like `:escape`, `:enter`, `:a`, `:digit_1`, `:arrow_left`, or `:plus`

  Raw key events stay layout-independent. Text-producing input is delivered separately
  through text commit/preedit events. For example, `Shift+=` reports raw key `:equal`
  with `[:shift]` and still commits the text `"+"`.

  ## Example

      EmergeSkia.set_input_target(renderer, self())

      receive do
        {:emerge_skia_event, {:cursor_button, {button, 1, _mods, {x, y}}}} ->
          IO.puts("Clicked \#{button} at \#{x}, \#{y}")

        {:emerge_skia_event, {:key, {key, 1, _mods}}} ->
          IO.puts("Key pressed: \#{key}")
      end
  """
  @spec set_input_target(renderer(), pid() | nil) :: :ok
  def set_input_target(renderer, pid) do
    renderer
    |> Transport.for_renderer()
    |> apply(:set_input_target, [renderer, pid])
  end

  @doc """
  Set the target process to receive native renderer log messages.

  Native logs are sent directly to the target process as
  `{:emerge_skia_log, level, source, message}` messages.
  """
  @spec set_log_target(renderer(), pid() | nil) :: :ok
  def set_log_target(renderer, pid) do
    renderer
    |> Transport.for_renderer()
    |> apply(:set_log_target, [renderer, pid])
  end

  @doc """
  Fetch renderer counters and timings as a structured snapshot.

  On native Wayland, DRM and headless renderers, collection is disabled by default.
  Set `diagnostics: [stats: true]` in `Emerge.Config` to collect data without
  periodic stats output, or `diagnostics: [logs: [:stats]]` to enable both
  collection and periodic summaries. See `Emerge.Config.Diagnostics` for their
  shared measurement-window behavior and log destinations.

      {:ok, snapshot} = EmergeSkia.stats(renderer, :peek)
      {:ok, closed_window} = EmergeSkia.stats(renderer, :take)

  Commands are:

  - `:peek` reads the current window without resetting it;
  - `:take` returns and resets the current window;
  - `:reset` discards the current window.

  When collection is disabled, the snapshot reports `enabled: false` and empty
  counters. Renderer collection is startup-only; `{:configure, %{enabled: boolean}}`
  is not supported for renderer handles.

  `:take` and `:reset` may return a draining error while asynchronous GPU samples
  finish; retry the same command to complete the closed window. The stats schema
  version is `25`. The macOS host returns `{:error, :not_supported}` for this API;
  use `diagnostics: [logs: [:stats]]` for its periodic summaries instead.
  """
  @spec stats(renderer(), Native.stats_command()) ::
          {:ok, Native.stats_snapshot()} | {:error, term()}
  def stats(renderer, command \\ :peek) do
    renderer
    |> Transport.for_renderer()
    |> apply(:stats, [renderer, command])
  end

  @doc """
  Fetch normalized renderer/backend information for a running renderer.

      {:ok, info} = EmergeSkia.renderer_info(renderer)
      info.rendering_api.selected
      info.capabilities.screenshot

  Explicit Vulkan renderers include the physical device that won startup
  selection. When selection used an exact DRM node, `vulkan_device.drm_node`
  contains its path, match field, and major/minor identity. The KMS card is not
  reported as the Vulkan node on split-device systems. Non-Vulkan renderers
  return `vulkan_device: nil`.

  Use `capabilities` to check capture, raster presentation, and PRIME video
  support before starting an optional operation.
  """
  @spec renderer_info(renderer()) :: {:ok, Native.renderer_info()} | {:error, term()}
  def renderer_info(renderer) do
    renderer
    |> Transport.for_renderer()
    |> apply(:renderer_info, [renderer])
  end
end
