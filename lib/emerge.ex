defmodule Emerge do
  @moduledoc """
  Defines Emerge viewport modules.

  A viewport owns a rendered UI and receives its events. Use `Emerge` in a
  module, implement `mount/1`, and implement either `render/0` or `render/1`.

  `use Emerge` imports the common `Emerge.UI` helpers and aliases `Emerge` as
  `Viewport` inside your module.

  ## Example

      defmodule MyApp.CounterViewport do
        use Emerge

        @impl Viewport
        def mount(opts) do
          output = %Emerge.Config.Window{title: Keyword.get(opts, :title, "Counter")}
          {:ok, %{count: 0}, %Emerge.Config{output: output}}
        end

        @impl Viewport
        def render(%{count: count}) do
          row([spacing(12), padding(12)], [
            Input.button([Event.on_press(:decrement)], text("-")),
            text("Count: \#{count}"),
            Input.button([Event.on_press(:increment)], text("+"))
          ])
        end

        @impl Viewport
        def handle_info(:increment, state) do
          {:noreply, Viewport.rerender(%{state | count: state.count + 1})}
        end

        def handle_info(:decrement, state) do
          {:noreply, Viewport.rerender(%{state | count: state.count - 1})}
        end
      end

  Start the viewport directly:

      {:ok, viewport} = MyApp.CounterViewport.start_link()

  Or supervise it:

      children = [MyApp.CounterViewport]
      Supervisor.start_link(children, strategy: :one_for_one)

  `use Emerge` adds `start_link/1` and `child_spec/1` to the viewport module.

  ## Callbacks

  `mount/1` receives the options passed to `start_link/1`. Return one of:

  - `{:ok, config}` and implement `render/0`;
  - `{:ok, state, config}` and implement `render/1`;
  - `{:stop, reason}` to stop startup.

  `config` must be `%Emerge.Config{}`. Application mount arguments remain keyword
  lists; they are not automatically merged into renderer configuration. State must
  be a map. The `:__emerge__` key is reserved.

  Optionally implement `config(previous)` or `config(previous, state)`, never both.
  The first invocation receives normalized mount config before renderer creation;
  later invocations receive the latest normalized desired intent, including while
  native application is pending or the renderer is recovering. Return a complete
  config; new nested records replace rather than merge with previous records.
  Invalid results retain previous intent; operational errors preserve desired
  intent for retry. See `Emerge.Config` for supported live versus startup-only fields.

  `render/0` or `render/1` returns an `Emerge.tree()` built with `Emerge.UI`.

  Implement `handle_info/2` for messages and element events. Implement
  `handle_input/2` only when you need raw renderer input. Implement
  `handle_close/2` to customize window-close behavior; the default stops the
  viewport normally.

  Callback handlers return `{:noreply, state}` or
  `{:stop, reason, state}`.

  ## Rerendering

  Call `rerender/1` after changing viewport state:

      {:noreply, Viewport.rerender(%{state | count: state.count + 1})}

  Return the state produced by `rerender/1`. It schedules a coalesced config/render
  cycle, not an immediate callback. Config-only changes do not require a tree diff.
  While DRM display intent is off, config reconciliation remains active but routine
  render callbacks and uploads are deferred; output ownership is still acquired.

  ## Element events

  Event helpers send ordinary Elixir messages. Passing only a message sends it
  to the viewport process:

      Input.button([Event.on_press(:save)], text("Save"))

      @impl Viewport
      def handle_info(:save, state) do
        {:noreply, state}
      end

  To send elsewhere, pass `{pid, message}`. Payload events append the payload to
  a tuple message or send `{message, payload}` for a non-tuple message. Override
  `wrap_payload/3` when an integration needs another shape. The rendering
  viewport's wrapper also applies to external recipients.

  `Event.on_click/1` and the five `on_mouse_*` helpers carry element-local
  `%{x: float(), y: float()}` positions. `Event.on_drag/1` adds a phase and opts
  into movement/release capture outside a custom control. `Event.on_press/1`
  remains payload-free and supports keyboard activation. These element events
  do not require raw input subscriptions. See `Emerge.UI.Event` for message
  shapes, coordinate rules and drag lifecycle.

  ## Viewport configuration

  A viewport's `%Emerge.Config{}` chooses its output, drawing API, input, assets,
  diagnostics and runtime settings. Return it from `c:mount/1`; direct renderer
  callers pass the same record to `EmergeSkia.start/1`. Each configuration record
  explains its fields and which can change while running.

  ### Native build support versus runtime selection

  `compiled_backends` belongs in `config/config.exs`, before compiling Emerge. It
  selects optional native support; `config.output` and `config.renderer.api` choose
  which available implementation a viewport uses at runtime.

  **Headless output and CPU raster rendering are always present.** DRM and Wayland
  add presenters; OpenGL and Vulkan add shared rendering APIs. Configuration entries
  resolve to a deduplicated union:

  | `compiled_backends` | Optional native features |
  | --- | --- |
  | `[]` or `[headless: [:raster]]` | None |
  | `[headless: [:opengl]]` | OpenGL |
  | `[drm: [:raster]]` | DRM |
  | `[drm: [:opengl]]` | DRM and OpenGL |
  | `[headless: [:opengl], drm: [:opengl]]` | The same DRM and OpenGL |
  | `[wayland: [:opengl]]` | Wayland and OpenGL |
  | `[drm: [:vulkan]]` | DRM and Vulkan |

  For example, the normal Linux desktop default can be made explicit:

  ```elixir
  # config/config.exs
  config :emerge, compiled_backends: [wayland: [:opengl]]
  ```

  APIs are shared build features, not separate per-backend exclusion masks.
  `[wayland: [:vulkan], drm: [:opengl]]` enables both presenters and both APIs.
  Repeated API selections do not create another artifact. Archives match the exact
  optional feature set: an `opengl` archive does not secretly include DRM/Wayland;
  `drm_opengl` includes DRM. The baseline archive has no variant suffix.
  Prebuilt coverage is curated per target for common configurations; accepting a
  configuration does not mean an archive is published for it. Other combinations
  require a source build, never an archive with a larger feature set.

  Use `:all` for the supported APIs on an entry. Existing atom shorthand remains:
  `[:drm]` and `[:wayland]` imply OpenGL, `[:macos]` implies Metal, and legacy
  `[:headless]` implies OpenGL. Use `[]` or `[headless: [:raster]]` for the minimal
  baseline. Explicit configuration always overrides defaults.

  Compiling an implementation does not choose a display, start a frame consumer or
  enable an API in an already-installed binary. Runtime config cannot add missing
  build features. Prefer explicit runtime APIs for hardware recipes: `:auto` is
  output-specific and does not promise to choose Vulkan merely because it is built.

  The canonical variants described here require 0.5.1 archives and matching
  checksums; existing 0.5.0 assets stay unchanged. An unreleased checkout needs an
  explicit source build rather than assuming its new archives already exist.

  ### Platform defaults

  When `compiled_backends` is unspecified:

  - Normal Linux uses `[wayland: [:opengl]]`.
  - macOS uses `[macos: [:metal]]` through the separate window host.
  - Nerves uses a known stock system/version capability profile; unknown/custom
    systems conservatively use headless/raster (`[]`).

  Headless/raster needs a live frame recipient, not the desktop window configuration
  used by the introductory counter. Metal remains on the macOS window host;
  headless Metal is deferred. Darwin headless raster uses its separate NIF.

  ### WSLg and minimal Linux desktops

  WSLg uses the Wayland window backend, not direct DRM/KMS. The normal Linux
  build includes OpenGL and therefore links GBM for the shared headless/PRIME
  implementation, even when the DRM presenter is not compiled. Source builds
  need matching Mesa runtime/development packages; disabling DRM alone does not
  remove this GBM requirement.

  If `mix compile` fails with `unable to find library -lgbm`, use matched distro
  packages or select a GBM-free software-rendered window build explicitly:

  ```elixir
  # config/config.exs, before compiling Emerge:
  config :emerge, compiled_backends: [wayland: [:raster]]
  ```

  ```elixir
  # mount/1 result (or EmergeSkia.start/1):
  %Emerge.Config{
    output: %Emerge.Config.Window{backend: :wayland},
    renderer: [api: :raster],
    assets: [otp_app: :my_app]
  }
  ```

  Rebuild with `mix deps.compile emerge --force`; source builds additionally use
  `EMERGE_SKIA_BUILD=true` and require Rustler. This route still needs Wayland,
  xkbcommon and a running WSLg compositor, but no GBM or GPU rendering API. It
  does not silently replace OpenGL or promise the same performance. Inspect the
  selected features with `mix emerge.build_info`, or before compilation with
  `elixir deps/emerge/scripts/build-info.exs --backend wayland:raster`.

  Wayland uses compositor-provided cursor shapes or the installed Xcursor theme.
  If neither the requested icon nor the theme's default exists, Emerge uses
  bundled cursors and reports the missing theme once per pointer/scale. To restore
  your preferred theme, check its installation and `XCURSOR_THEME`, `XCURSOR_SIZE`
  and `XCURSOR_PATH` in the environment that launches the application.

  ### Nerves and device defaults

  Choose the highest usable tier of the **image and display path**, not just the
  SoC's advertised API:

  | Established support | Build setting | Runtime API |
  | --- | --- | --- |
  | Atomic KMS and a usable Vulkan stack | `[drm: [:vulkan]]` | `:vulkan` with a device |
  | Atomic KMS and usable OpenGL ES | `[drm: [:opengl]]` | `:opengl` |
  | Atomic KMS with CPU scanout, no usable GPU renderer | `[drm: [:raster]]` | `:raster` |
  | No usable atomic KMS, but offscreen EGL/GLES | `[headless: [:opengl]]` | `:opengl` with a frame consumer |
  | No established path, or unknown system | `[]` | `:raster` with a frame consumer |

  DRM/raster also requires atomic KMS, dumb buffers, a compatible XRGB8888 plane
  and leasing. It is not a legacy-KMS/fbdev fallback. Headless Vulkan currently
  exports PRIME frames, not ordinary binary pixels; raster cannot export PRIME.

  Known defaults match Nerves' exported `NERVES_SYSTEM` package/version, such as
  `nerves_system_rpi5-portable-2.0.1`, not a `MIX_TARGET` nickname or CPU architecture.
  The initial profiles describe stock-image configuration, not physical-board
  qualification. They deliberately do not guess about custom or future images:

  #{EmergeSkia.BuildInfo.system_table()}

  These stock Pi 4/5 images configure KMS and Mesa GLES but not a Vulkan stack.
  Hardware Vulkan support does not install the required loader/driver/extensions.
  Older stock images without Mesa do not mean the SoC can never render: a custom
  image may supply a usable EGL/GLES stack. Override the build explicitly when
  using such an image, or an SPI panel instead of the board's KMS/HDMI path.

  ARMv6 still requires a source build. Determine 32-bit/64-bit ABI from the active
  system/toolchain, not the board name. GPU rendering still needs suitable target
  libraries and device access; neither artifact availability nor NIF loading proves
  hardware support. Build inspection never probes the build host's GPU.

  ### Runtime output examples

  These records can be returned from `c:mount/1` or passed to `EmergeSkia.start/1`.
  Replace `:my_app` and device paths with your application/system values. A viewport
  can infer `assets.otp_app`; standalone renderer callers must provide it.

  For the Wayland/OpenGL build above:

  ```elixir
  %Emerge.Config{
    output: %Emerge.Config.Window{backend: :wayland, title: "My app"},
    renderer: [api: :opengl],
    assets: [otp_app: :my_app]
  }
  ```

  For `compiled_backends: [drm: [:opengl]]`:

  ```elixir
  %Emerge.Config{
    output: %Emerge.Config.DRM{card: "/dev/dri/card0"},
    renderer: [api: :opengl],
    assets: [otp_app: :my_app]
  }
  ```

  For `compiled_backends: [drm: [:vulkan]]`, select a compatible GPU device as well
  as the display card; these paths are examples, not universal numbering:

  ```elixir
  %Emerge.Config{
    output: %Emerge.Config.DRM{card: "/dev/dri/card0"},
    renderer: [api: :vulkan, device: "/dev/dri/renderD128"],
    assets: [otp_app: :my_app]
  }
  ```

  For `compiled_backends: [drm: [:raster]]`, use CPU KMS presentation without a GPU
  stack. GPU-upload raster is a separate explicit OpenGL-backed route:

  ```elixir
  %Emerge.Config{
    output: %Emerge.Config.DRM{card: "/dev/dri/card0"},
    renderer: [api: :raster, raster_present: :cpu],
    assets: [otp_app: :my_app]
  }
  ```

  For `compiled_backends: [headless: [:opengl]]`, construct the config in the live
  frame-consumer process (or supply that process's PID instead of `self()`):

  ```elixir
  %Emerge.Config{
    output: %Emerge.Config.Headless{
      target: self(),
      size: {320, 240},
      frames: %Emerge.Config.BinaryFrames{pixel_format: :rgb888}
    },
    renderer: [api: :opengl],
    assets: [otp_app: :my_app]
  }
  ```

  The recipient handles `{:emerge_skia_frame, %VideoInterop.Frame{}}` and sends pixels
  to its panel. Honor the format/stride and do panel-specific conversion, such as
  RGB888 to RGB565 for an SPI/ILI9341 driver, in your application. Owned binary frames
  need no release; PRIME has a different ownership protocol. This headless GL build
  needs EGL/GBM/GLES and GPU device access, but no DRM/KMS or Wayland presenter.

  For the minimal `compiled_backends: []` build, use raster instead:

  ```elixir
  %Emerge.Config{
    output: %Emerge.Config.Headless{
      target: self(),
      size: {320, 240},
      frames: %Emerge.Config.BinaryFrames{pixel_format: :rgb888}
    },
    renderer: [api: :raster],
    assets: [otp_app: :my_app]
  }
  ```

  ### Inspect and rebuild native support

  Once the dependency's Elixir code is available:

  ```sh
  MIX_TARGET=your_target mix emerge.build_info
  mix emerge.build_info --systems
  mix emerge.build_info --profiles
  ```

  For a desktop, omit `MIX_TARGET`. Before native compilation succeeds, the shipped
  standalone script needs no NIF, Rustler or compiled dependency code:

  ```sh
  elixir deps/emerge/scripts/build-info.exs \\
    --backend headless:opengl --target aarch64-unknown-linux-gnu
  ```

  In Emerge's own checkout, omit `deps/emerge/`. Without explicit `--backend` values,
  the standalone script reports environment defaults, not unloaded application
  config. Inspect firmware inside the bootstrapped Nerves environment so the system
  and target ABI are available. The report describes the requested build, not a
  previously installed binary, and does not assert that an archive is published.

  After changing build settings, the system/version, driver stack or ABI:

  ```sh
  mix deps.compile emerge --force
  # Firmware:
  MIX_TARGET=your_target mix deps.compile emerge --force
  MIX_TARGET=your_target mix firmware
  ```

  Unsupported exact profiles or missing checksums are explained before a potentially
  long native fallback. To deliberately build from source, add the optional build
  dependency to your application:

  ```elixir
  {:rustler, "~> 0.38.0", runtime: false}
  ```

  ```sh
  EMERGE_SKIA_BUILD=true MIX_TARGET=your_target mix deps.compile emerge --force
  ```

  Source builds need Elixir 1.19+, Rust 1.91+, a C/C++ toolchain and the selected
  target libraries. Use Nerves' active cross compiler/sysroot, never a host-built
  NIF in firmware. rust-skia uses host Python; set `EMERGE_SKIA_HOST_PYTHON` to an
  absolute host executable if `/usr/bin/python3` is unsuitable. Corrupt archives or
  invalid checksum manifests are errors, not permission to bypass integrity.

  ### Live configuration and events

  Return `%Emerge.Config{}` from mount. Optional `c:config/1` or `c:config/2`
  callbacks receive the previous normalized desired config and reconcile changes
  during rerenders. Each `Emerge.Config` record documents its live fields and backend
  limits. A complete live config is distinct from native application or presentation
  completion.

  Raw input subscriptions use `Emerge.Config.Input.events`, defaulting to `[]`;
  see `Emerge.Input` and the [input guide](handle_input.html). This filter does not
  disable element interactions. Video targets are viewport-local atoms; submit
  owned binary or borrowed DMA-BUF frames with `submit_video_frame/3`.
  """

  alias Emerge.Runtime.Viewport, as: RuntimeViewport

  @typedoc "Public tree type built with `Emerge.UI` and rendered by Emerge backends."
  @type tree :: Emerge.Engine.Element.t()

  @typedoc "Viewport state map passed to render/1 and callback functions."
  @type state :: map()

  @callback mount(keyword()) ::
              {:ok, state(), Emerge.Config.t()} | {:ok, Emerge.Config.t()} | {:stop, term()}

  @doc "Returns desired configuration, given the previous normalized config."
  @callback config(Emerge.Config.normalized_t()) :: Emerge.Config.t()
  @doc "Returns desired configuration, given the previous normalized config and application state."
  @callback config(Emerge.Config.normalized_t(), state()) :: Emerge.Config.t()
  @callback render() :: tree()
  @callback render(state()) :: tree()

  @callback handle_info(term(), state()) ::
              {:noreply, state()} | {:stop, term(), state()}

  @callback handle_input(Emerge.Input.event(), state()) ::
              {:noreply, state()} | {:stop, term(), state()}

  @callback handle_close(term(), state()) ::
              {:noreply, state()} | {:stop, term(), state()}

  @doc """
  Wraps a runtime element payload into its stored message before delivery.

  Runs in the rendering viewport, including for external-PID recipients. `kind`
  identifies the event, such as `:change`, `:scroll`, `:click`, `:mouse_move` or
  `:drag`; `Emerge.UI.Event` documents the payloads. Payload-free events bypass
  this callback, including `on_press`, keyboard and focus/blur handlers.

  The default appends the payload to tuple messages and otherwise returns
  `{message, payload}`. Delegate unmatched cases to `default_wrap_payload/3`.
  """
  @callback wrap_payload(message :: term(), payload :: term(), kind :: term()) :: term()

  @optional_callbacks config: 1,
                      config: 2,
                      render: 0,
                      render: 1,
                      handle_info: 2,
                      handle_input: 2,
                      handle_close: 2,
                      wrap_payload: 3

  defmacro __using__(_opts) do
    quote do
      use Emerge.UI
      alias Emerge, as: Viewport

      @behaviour Emerge

      def start_link(opts \\ []) do
        Emerge.Runtime.Viewport.start_link(__MODULE__, opts)
      end

      def child_spec(opts) do
        Emerge.Runtime.Viewport.child_spec(__MODULE__, opts)
      end

      @impl Emerge
      def handle_input(event, state)

      def handle_input(_event, state), do: {:noreply, state}

      @impl Emerge
      def handle_close(_reason, state), do: {:stop, :normal, state}

      @impl Emerge
      def wrap_payload(message, payload, event_type) do
        Emerge.default_wrap_payload(message, payload, event_type)
      end

      defoverridable handle_input: 2, handle_close: 2, wrap_payload: 3
    end
  end

  @doc """
  Requests a rerender from viewports after application source is reloaded.

  `Emerge.Runtime.CodeReloader` calls this automatically. Custom development
  reloaders may pass metadata for their own logging or coordination.
  """
  @spec notify_source_reloaded(term()) :: :ok
  def notify_source_reloaded(meta \\ %{}) do
    RuntimeViewport.notify_source_reloaded(meta)
  end

  @doc """
  Returns the renderer handle owned by a running viewport.

  Use this handle with renderer-specific APIs such as
  `EmergeSkia.renderer_info/1`, `EmergeSkia.stats/2`, and capture.

      renderer = Emerge.renderer(viewport)
      {:ok, info} = EmergeSkia.renderer_info(renderer)
  """
  @spec renderer(pid()) :: term()
  def renderer(pid) when is_pid(pid) do
    RuntimeViewport.renderer(pid)
  end

  @doc """
  Submits one frame to a viewport-local video target.

  The target is the same atom passed to `Emerge.UI.video/2`. Hidden targets
  consume and drop frames. Visible targets retain only the latest frame.

  Every normal return consumes the supplied frame: immutable binary frames need
  no further action, while borrowed storage is released by Emerge when retired.

      :ok = Emerge.submit_video_frame(viewport, :camera, frame)
  """
  @spec submit_video_frame(pid(), atom(), VideoInterop.Frame.t()) :: :ok | {:error, term()}
  def submit_video_frame(viewport, target, %VideoInterop.Frame{} = frame)
      when is_pid(viewport) and is_atom(target) do
    Emerge.Runtime.VideoEndpoints.submit(viewport, target, frame)
  end

  @doc """
  Marks viewport state for rerendering and returns the updated state.

  Call this after changing local state and return its result from the callback:

      {:noreply, Viewport.rerender(%{state | count: state.count + 1})}
  """
  @spec rerender(state()) :: state()
  def rerender(state) when is_map(state) do
    RuntimeViewport.rerender(state)
  end

  @doc """
  Applies Emerge's default payload-event message shape.

  Tuple messages receive the payload as a final tuple element. Other messages
  become `{message, payload}`. Viewports normally use this through the default
  `wrap_payload/3` callback.
  """
  @spec default_wrap_payload(term(), term(), term()) :: term()
  def default_wrap_payload(message, payload, event_type) do
    RuntimeViewport.default_wrap_payload(message, payload, event_type)
  end
end
