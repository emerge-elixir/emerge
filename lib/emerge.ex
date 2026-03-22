defmodule Emerge do
  @moduledoc """
  Root public API for building and running Emerge UIs.

  Build trees with `Emerge.UI`, run viewport processes with `use Emerge`,
  and use retained-tree helpers through `Emerge.Engine`.
  """

  alias Emerge.Runtime.Viewport, as: RuntimeViewport
  alias Emerge.Runtime.Viewport.State

  @typedoc "Public tree type built with `Emerge.UI` and rendered by Emerge backends."
  @opaque tree :: %{
            type: atom(),
            id: term() | nil,
            attrs: map(),
            children: [tree()],
            frame:
              %{
                x: number(),
                y: number(),
                width: number(),
                height: number()
              }
              | nil
          }

  @typedoc "Opaque viewport runtime state."
  @opaque viewport_state :: %{
            module: module(),
            mount_opts: keyword(),
            user_state: term(),
            renderer: term() | nil,
            diff_state: Emerge.Engine.diff_state() | nil,
            dirty?: boolean(),
            flush_scheduled?: boolean(),
            renderer_module: module(),
            renderer_opts: keyword(),
            skia_opts: keyword(),
            input_mask: non_neg_integer() | nil,
            renderer_check_interval_ms: non_neg_integer() | nil
          }

  @callback mount(keyword()) :: {:ok, term(), keyword()} | {:stop, term()}
  @callback render(term()) :: tree()

  @callback handle_input(term(), term()) ::
              {:ok, term()} | {:ok, term(), keyword()} | {:stop, term(), term()}

  @callback wrap_payload(term(), term(), atom()) :: term()

  @optional_callbacks handle_input: 2, wrap_payload: 3

  defmacro __using__(_opts) do
    quote do
      use Emerge.UI
      alias Emerge, as: Viewport

      @behaviour GenServer
      @behaviour Emerge

      def start_link(opts \\ []) do
        Emerge.Runtime.Viewport.__start_link__(__MODULE__, opts)
      end

      def child_spec(opts) do
        Emerge.Runtime.Viewport.__child_spec__(__MODULE__, opts)
      end

      @impl true
      def init(opts) do
        Emerge.Runtime.Viewport.__init__(__MODULE__, opts)
      end

      @impl true
      def handle_continue({:emerge_viewport_mount, opts}, state) do
        Emerge.Runtime.Viewport.__handle_continue_mount__(opts, state)
      end

      @impl true
      def handle_info({:emerge_skia_event, event}, state) do
        Emerge.Runtime.Viewport.__handle_skia_event__(event, state)
      end

      @impl true
      def handle_info({:emerge_viewport, :check_renderer}, state) do
        Emerge.Runtime.Viewport.__handle_check_renderer__(state)
      end

      @impl true
      def handle_info({:emerge_viewport, :source_reloaded, meta}, state) do
        Emerge.Runtime.Viewport.__handle_source_reloaded__(meta, state)
      end

      @impl true
      def handle_cast({:emerge_viewport, :flush}, state) do
        Emerge.Runtime.Viewport.__handle_flush__(state)
      end

      @impl true
      def handle_cast({:emerge_viewport, :rerender}, state) do
        {:noreply, Viewport.schedule_rerender(state)}
      end

      @impl true
      def handle_call({:emerge_viewport, :renderer}, _from, state) do
        {:reply, state.renderer, state}
      end

      @impl true
      def handle_call({:emerge_viewport, :user_state}, _from, state) do
        {:reply, Viewport.user_state(state), state}
      end

      @impl true
      def handle_call({:emerge_viewport, :rerender}, _from, state) do
        {:reply, :ok, Viewport.schedule_rerender(state)}
      end

      @impl Emerge
      def handle_input(_event, user_state), do: {:ok, user_state, rerender: false}

      @impl Emerge
      def wrap_payload(message, payload, event_type) do
        Viewport.default_wrap_payload(message, payload, event_type)
      end

      @impl true
      def terminate(reason, state) do
        Emerge.Runtime.Viewport.__terminate__(reason, state)
      end

      defoverridable start_link: 1,
                     child_spec: 1,
                     init: 1,
                     handle_continue: 2,
                     handle_input: 2,
                     wrap_payload: 3,
                     terminate: 2
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

  @spec rerender(pid()) :: :ok
  def rerender(pid) when is_pid(pid) do
    RuntimeViewport.rerender(pid)
  end

  @spec user_state(viewport_state()) :: term()
  def user_state(%State{} = state), do: RuntimeViewport.user_state(state)

  @spec user_state(pid()) :: term()
  def user_state(pid) when is_pid(pid) do
    RuntimeViewport.user_state(pid)
  end

  @spec update_user_state(viewport_state(), (term() -> term())) :: viewport_state()
  def update_user_state(%State{} = state, fun) when is_function(fun, 1) do
    RuntimeViewport.update_user_state(state, fun)
  end

  @spec schedule_rerender(viewport_state()) :: viewport_state()
  def schedule_rerender(%State{} = state) do
    RuntimeViewport.schedule_rerender(state)
  end

  @spec default_wrap_payload(term(), term(), atom()) :: term()
  def default_wrap_payload(message, payload, event_type) when is_atom(event_type) do
    RuntimeViewport.default_wrap_payload(message, payload, event_type)
  end
end
