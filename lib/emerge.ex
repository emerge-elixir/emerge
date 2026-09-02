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
          {:ok, %{count: 0}, Keyword.merge([title: "Counter"], opts)}
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

  - `{:ok, viewport_options}` and implement `render/0`;
  - `{:ok, state, viewport_options}` and implement `render/1`;
  - `{:stop, reason}` to stop startup.

  State must be a map. The `:__emerge__` key is reserved.

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

  Return the state produced by `rerender/1`. Calling it schedules a new render;
  it does not call `render/1` immediately.

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
  `wrap_payload/3` when an integration needs another shape.

  ## Configuration and output modes

  Viewport options choose the window/display backend, rendering API, size,
  assets, diagnostics, and headless output. `Emerge.Runtime.Viewport` documents
  these user-facing options and includes desktop, DRM, and headless examples.

  Options can be returned at the top level or under `emerge_skia: [...]`.
  Runtime-only viewport options use `viewport: [...]`.

  Video elements use viewport-local atom targets. Submit owned binary or
  borrowed DMA-BUF frames with `submit_video_frame/3`. Vulkan rendering and
  Vulkan video import remain experimental.
  """

  alias Emerge.Runtime.Viewport, as: RuntimeViewport

  @typedoc "Public tree type built with `Emerge.UI` and rendered by Emerge backends."
  @type tree :: Emerge.Engine.Element.t()

  @typedoc "Viewport state map passed to render/1 and callback functions."
  @type state :: map()

  @callback mount(keyword()) :: {:ok, state(), keyword()} | {:ok, keyword()} | {:stop, term()}
  @callback render() :: tree()
  @callback render(state()) :: tree()

  @callback handle_info(term(), state()) ::
              {:noreply, state()} | {:stop, term(), state()}

  @callback handle_input(term(), state()) ::
              {:noreply, state()} | {:stop, term(), state()}

  @callback handle_close(term(), state()) ::
              {:noreply, state()} | {:stop, term(), state()}

  @callback wrap_payload(term(), term(), term()) :: term()

  @optional_callbacks render: 0,
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
