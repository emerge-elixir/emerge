defmodule Emerge do
  @moduledoc """
  Public API for writing viewport modules.

  Use `Emerge` for modules that mount viewport state or options, define UI with
  `render/0` or `render/1`, handle input, and request rerenders.

  `use Emerge` also brings the common `Emerge.UI` helpers into scope, so
  viewport modules can declare trees directly or call regular Elixir functions
  that return `Emerge.tree()`.

  It also aliases `Emerge` as `Viewport`, which makes callbacks and helper
  calls such as `@impl Viewport` and `Viewport.rerender(state)` available
  inside the module.

  Viewport state is a plain map. Emerge keeps its runtime metadata under the
  reserved `:__emerge__` key.

  Element event helpers such as `Event.on_press/1`, `Event.on_click/1`, and
  `Event.on_swipe_right/1`
  deliver regular process messages and are usually handled in `handle_info/2`.
  Implement `handle_input/2` when you want to react to raw input events and
  lifecycle notifications coming from the renderer.

  `use Emerge` stops the viewport by default when `handle_input/2` receives
  `:closed`. If you override `handle_input/2`, match `:closed` yourself or
  delegate unmatched events to `super/2` to keep that behavior.

  For retained-tree diffing, encoding, and event routing helpers, see
  `Emerge.Engine`.
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

  @callback wrap_payload(term(), term(), term()) :: term()

  @optional_callbacks render: 0, render: 1, handle_info: 2, handle_input: 2, wrap_payload: 3

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

      def handle_input(:closed, state), do: {:stop, :normal, state}
      def handle_input(_event, state), do: {:noreply, state}

      @impl Emerge
      def wrap_payload(message, payload, event_type) do
        Emerge.default_wrap_payload(message, payload, event_type)
      end

      defoverridable handle_input: 2, wrap_payload: 3
    end
  end

  @spec notify_source_reloaded(term()) :: :ok
  def notify_source_reloaded(meta \\ %{}) do
    RuntimeViewport.notify_source_reloaded(meta)
  end

  @spec renderer(pid()) :: term()
  def renderer(pid) when is_pid(pid) do
    RuntimeViewport.renderer(pid)
  end

  @spec rerender(state()) :: state()
  def rerender(state) when is_map(state) do
    RuntimeViewport.rerender(state)
  end

  @spec default_wrap_payload(term(), term(), term()) :: term()
  def default_wrap_payload(message, payload, event_type) do
    RuntimeViewport.default_wrap_payload(message, payload, event_type)
  end
end
