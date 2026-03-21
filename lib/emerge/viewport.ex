defmodule Emerge.Viewport do
  @moduledoc """
  Generic GenServer runtime for Emerge viewport processes.

  A viewport process owns a renderer instance, uploads an initial tree, patches future
  trees, and routes `{:emerge_skia_event, ...}` messages into user/application messages.

  The module is intentionally state-management agnostic.
  """

  alias Emerge.Viewport.Renderer.Skia

  @genserver_start_options [:name, :timeout, :debug, :spawn_opt, :hibernate_after]
  @default_renderer_check_interval_ms 500

  defmodule State do
    @moduledoc false

    @enforce_keys [:module, :mount_opts]
    defstruct module: nil,
              mount_opts: [],
              user_state: nil,
              renderer: nil,
              diff_state: nil,
              dirty?: false,
              flush_scheduled?: false,
              renderer_module: Emerge.Viewport.Renderer.Skia,
              renderer_opts: [],
              skia_opts: [],
              input_mask: nil,
              renderer_check_interval_ms: 500

    @type t :: %__MODULE__{
            module: module(),
            mount_opts: keyword(),
            user_state: term(),
            renderer: term() | nil,
            diff_state: Emerge.DiffState.t() | nil,
            dirty?: boolean(),
            flush_scheduled?: boolean(),
            renderer_module: module(),
            renderer_opts: keyword(),
            skia_opts: keyword(),
            input_mask: non_neg_integer() | nil,
            renderer_check_interval_ms: non_neg_integer() | nil
          }
  end

  @type t :: State.t()

  @callback mount(keyword()) :: {:ok, term(), keyword()} | {:stop, term()}
  @callback render(term()) :: Emerge.Element.t()

  @callback handle_input(term(), term()) ::
              {:ok, term()} | {:ok, term(), keyword()} | {:stop, term(), term()}

  @callback wrap_payload(term(), term(), atom()) :: term()

  @optional_callbacks handle_input: 2, wrap_payload: 3

  defmacro __using__(_opts) do
    quote do
      import Emerge.UI
      alias Emerge.UI.Input
      alias Emerge.Viewport, as: Viewport

      @behaviour GenServer
      @behaviour Emerge.Viewport

      def start_link(opts \\ []) do
        Viewport.__start_link__(__MODULE__, opts)
      end

      def child_spec(opts) do
        Viewport.__child_spec__(__MODULE__, opts)
      end

      @impl true
      def init(opts) do
        Viewport.__init__(__MODULE__, opts)
      end

      @impl true
      def handle_continue({:emerge_viewport_mount, opts}, state) do
        Viewport.__handle_continue_mount__(opts, state)
      end

      @impl true
      def handle_info({:emerge_skia_event, event}, state) do
        Emerge.Viewport.__handle_skia_event__(event, state)
      end

      @impl true
      def handle_info({:emerge_viewport, :check_renderer}, state) do
        Emerge.Viewport.__handle_check_renderer__(state)
      end

      @impl true
      def handle_cast({:emerge_viewport, :flush}, state) do
        Emerge.Viewport.__handle_flush__(state)
      end

      @impl true
      def handle_cast({:emerge_viewport, :rerender}, state) do
        {:noreply, Emerge.Viewport.schedule_rerender(state)}
      end

      @impl true
      def handle_call({:emerge_viewport, :renderer}, _from, state) do
        {:reply, state.renderer, state}
      end

      @impl true
      def handle_call({:emerge_viewport, :user_state}, _from, state) do
        {:reply, Emerge.Viewport.user_state(state), state}
      end

      @impl true
      def handle_call({:emerge_viewport, :rerender}, _from, state) do
        {:reply, :ok, Emerge.Viewport.schedule_rerender(state)}
      end

      @impl Emerge.Viewport
      def handle_input(_event, user_state), do: {:ok, user_state, rerender: false}

      @impl Emerge.Viewport
      def wrap_payload(message, payload, event_type) do
        Viewport.default_wrap_payload(message, payload, event_type)
      end

      @impl true
      def terminate(reason, state) do
        Viewport.__terminate__(reason, state)
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

  @doc false
  @spec __start_link__(module(), keyword()) :: GenServer.on_start()
  def __start_link__(module, opts) when is_atom(module) and is_list(opts) do
    genserver_opts = Keyword.take(opts, @genserver_start_options)
    init_opts = Keyword.drop(opts, @genserver_start_options)
    GenServer.start_link(module, init_opts, genserver_opts)
  end

  @doc false
  @spec __child_spec__(module(), keyword()) :: map()
  def __child_spec__(module, opts) when is_atom(module) and is_list(opts) do
    %{
      id: Keyword.get(opts, :name, module),
      start: {module, :start_link, [opts]},
      restart: :transient,
      type: :worker
    }
  end

  @doc false
  @spec __init__(module(), keyword()) ::
          {:ok, t(), {:continue, {:emerge_viewport_mount, keyword()}}}
  def __init__(module, opts) when is_atom(module) and is_list(opts) do
    {:ok,
     %State{
       module: module,
       mount_opts: opts
     }, {:continue, {:emerge_viewport_mount, opts}}}
  end

  @doc false
  @spec __handle_continue_mount__(keyword(), t()) :: {:noreply, t()} | {:stop, term(), t()}
  def __handle_continue_mount__(opts, %State{} = state) when is_list(opts) do
    case apply(state.module, :mount, [opts]) do
      {:ok, user_state, mount_opts} when is_list(mount_opts) ->
        mount_config = parse_mount_config!(state.module, mount_opts)

        case mount_config.renderer_module.start(
               mount_config.skia_opts,
               mount_config.renderer_opts
             ) do
          {:ok, renderer} ->
            :ok = mount_config.renderer_module.set_input_target(renderer, self())

            if is_integer(mount_config.input_mask) do
              :ok = mount_config.renderer_module.set_input_mask(renderer, mount_config.input_mask)
            end

            tree = apply(state.module, :render, [user_state])

            {diff_state, _assigned} = mount_config.renderer_module.upload_tree(renderer, tree)

            state =
              %{
                state
                | user_state: user_state,
                  renderer: renderer,
                  diff_state: diff_state,
                  renderer_module: mount_config.renderer_module,
                  renderer_opts: mount_config.renderer_opts,
                  skia_opts: mount_config.skia_opts,
                  input_mask: mount_config.input_mask,
                  renderer_check_interval_ms: mount_config.renderer_check_interval_ms
              }
              |> maybe_schedule_renderer_check()

            {:noreply, state}

          {:error, reason} ->
            {:stop, {:renderer_start_failed, reason}, state}

          other ->
            {:stop, {:unexpected_renderer_start_result, other}, state}
        end

      {:stop, reason} ->
        {:stop, reason, state}

      other ->
        raise ArgumentError,
              "#{inspect(state.module)}.mount/1 must return {:ok, user_state, opts} or {:stop, reason}, got: #{inspect(other)}"
    end
  end

  @doc false
  @spec __handle_skia_event__(term(), t()) :: {:noreply, t()} | {:stop, term(), t()}
  def __handle_skia_event__(event, %State{} = state) do
    case event do
      {id_bin, event_type} when is_binary(id_bin) and is_atom(event_type) ->
        route_element_event(state, id_bin, event_type, :no_payload)
        {:noreply, state}

      {id_bin, event_type, payload} when is_binary(id_bin) and is_atom(event_type) ->
        route_element_event(state, id_bin, event_type, {:with_payload, payload})
        {:noreply, state}

      _ ->
        state.module
        |> apply(:handle_input, [event, state.user_state])
        |> apply_user_state_result(state)
    end
  end

  @doc false
  @spec __handle_check_renderer__(t()) :: {:noreply, t()} | {:stop, term(), t()}
  def __handle_check_renderer__(%State{renderer: nil} = state), do: {:noreply, state}

  def __handle_check_renderer__(%State{} = state) do
    if state.renderer_module.running?(state.renderer) do
      {:noreply, maybe_schedule_renderer_check(state)}
    else
      {:stop, :normal, state}
    end
  end

  @doc false
  @spec __handle_flush__(t()) :: {:noreply, t()}
  def __handle_flush__(%State{} = state) do
    state = %{state | flush_scheduled?: false}

    if (state.dirty? and state.renderer) && state.diff_state do
      tree = apply(state.module, :render, [state.user_state])

      {diff_state, _assigned} =
        state.renderer_module.patch_tree(state.renderer, state.diff_state, tree)

      {:noreply, %{state | dirty?: false, diff_state: diff_state}}
    else
      {:noreply, state}
    end
  end

  @doc false
  @spec __terminate__(term(), t()) :: :ok
  def __terminate__(_reason, %State{renderer: nil}), do: :ok

  def __terminate__(_reason, %State{} = state) do
    _ = safe_stop_renderer(state.renderer_module, state.renderer)
    :ok
  end

  @spec renderer(pid()) :: term()
  def renderer(pid) when is_pid(pid) do
    GenServer.call(pid, {:emerge_viewport, :renderer})
  end

  @spec rerender(pid()) :: :ok
  def rerender(pid) when is_pid(pid) do
    GenServer.call(pid, {:emerge_viewport, :rerender})
  end

  @spec user_state(t()) :: term()
  def user_state(%State{user_state: user_state}), do: user_state

  @spec user_state(pid()) :: term()
  def user_state(pid) when is_pid(pid) do
    GenServer.call(pid, {:emerge_viewport, :user_state})
  end

  @spec put_user_state(t(), term()) :: t()
  def put_user_state(%State{} = state, user_state), do: %{state | user_state: user_state}

  @spec update_user_state(t(), (term() -> term())) :: t()
  def update_user_state(%State{} = state, fun) when is_function(fun, 1) do
    put_user_state(state, fun.(state.user_state))
  end

  @spec schedule_rerender(t()) :: t()
  def schedule_rerender(%State{} = state) do
    state = %{state | dirty?: true}

    if state.flush_scheduled? do
      state
    else
      GenServer.cast(self(), {:emerge_viewport, :flush})
      %{state | flush_scheduled?: true}
    end
  end

  @spec default_wrap_payload(term(), term(), atom()) :: term()
  def default_wrap_payload(message, payload, _event_type) when is_tuple(message) do
    Tuple.insert_at(message, tuple_size(message), payload)
  end

  def default_wrap_payload(message, payload, _event_type), do: {message, payload}

  defp route_element_event(%State{diff_state: nil}, _id_bin, _event_type, _payload_mode), do: :ok

  defp route_element_event(%State{} = state, id_bin, event_type, payload_mode) do
    case Emerge.lookup_event(state.diff_state, id_bin, event_type) do
      {:ok, {pid, message}} when is_pid(pid) ->
        routed_message =
          case payload_mode do
            :no_payload ->
              message

            {:with_payload, payload} ->
              apply(state.module, :wrap_payload, [message, payload, event_type])
          end

        send(pid, routed_message)

      _ ->
        :ok
    end
  end

  defp apply_user_state_result({:ok, user_state}, %State{} = state) do
    {:noreply, state |> put_user_state(user_state) |> schedule_rerender()}
  end

  defp apply_user_state_result({:ok, user_state, opts}, %State{} = state) when is_list(opts) do
    state = put_user_state(state, user_state)

    if Keyword.get(opts, :rerender, true) do
      {:noreply, schedule_rerender(state)}
    else
      {:noreply, state}
    end
  end

  defp apply_user_state_result({:stop, reason, user_state}, %State{} = state) do
    {:stop, reason, put_user_state(state, user_state)}
  end

  defp apply_user_state_result(other, %State{} = state) do
    raise ArgumentError,
          "#{inspect(state.module)}.handle_input/2 must return {:ok, state}, {:ok, state, opts}, or {:stop, reason, state}, got: #{inspect(other)}"
  end

  defp maybe_schedule_renderer_check(%State{renderer_check_interval_ms: interval} = state)
       when is_integer(interval) and interval > 0 do
    Process.send_after(self(), {:emerge_viewport, :check_renderer}, interval)
    state
  end

  defp maybe_schedule_renderer_check(%State{} = state), do: state

  defp safe_stop_renderer(renderer_module, renderer) do
    renderer_module.stop(renderer)
  catch
    _kind, _reason -> :ok
  end

  defp parse_mount_config!(module, opts) when is_atom(module) and is_list(opts) do
    explicit_skia_opts =
      case Keyword.fetch(opts, :emerge_skia) do
        {:ok, value} when is_list(value) ->
          value

        {:ok, other} ->
          raise ArgumentError,
                "mount/1 emerge_skia option must be a keyword list, got: #{inspect(other)}"

        :error ->
          []
      end

    bare_skia_opts = Keyword.drop(opts, [:emerge_skia, :viewport])

    skia_opts =
      bare_skia_opts
      |> Keyword.merge(explicit_skia_opts)
      |> ensure_skia_otp_app!(module)

    viewport_opts =
      case Keyword.get(opts, :viewport, []) do
        value when is_list(value) ->
          value

        other ->
          raise ArgumentError,
                "mount/1 viewport option must be a keyword list, got: #{inspect(other)}"
      end

    renderer_module = Keyword.get(viewport_opts, :renderer_module, Skia)

    unless is_atom(renderer_module) do
      raise ArgumentError,
            "viewport renderer_module must be a module, got: #{inspect(renderer_module)}"
    end

    case Code.ensure_loaded(renderer_module) do
      {:module, _module} ->
        :ok

      {:error, reason} ->
        raise ArgumentError,
              "viewport renderer_module #{inspect(renderer_module)} could not be loaded: #{inspect(reason)}"
    end

    required_renderer_callbacks = [
      start: 2,
      stop: 1,
      running?: 1,
      set_input_target: 2,
      set_input_mask: 2,
      upload_tree: 2,
      patch_tree: 3
    ]

    missing_renderer_callbacks =
      required_renderer_callbacks
      |> Enum.reject(fn {name, arity} -> function_exported?(renderer_module, name, arity) end)
      |> Enum.map_join(", ", fn {name, arity} -> "#{name}/#{arity}" end)

    unless missing_renderer_callbacks == "" do
      raise ArgumentError,
            "viewport renderer_module #{inspect(renderer_module)} must implement Emerge.Viewport.Renderer callbacks (missing: #{missing_renderer_callbacks})"
    end

    renderer_opts = Keyword.get(viewport_opts, :renderer_opts, [])

    unless is_list(renderer_opts) do
      raise ArgumentError,
            "viewport renderer_opts must be a keyword list, got: #{inspect(renderer_opts)}"
    end

    input_mask = Keyword.get(viewport_opts, :input_mask, nil)

    unless is_nil(input_mask) or (is_integer(input_mask) and input_mask >= 0) do
      raise ArgumentError,
            "viewport input_mask must be nil or a non-negative integer, got: #{inspect(input_mask)}"
    end

    renderer_check_interval_ms =
      Keyword.get(viewport_opts, :renderer_check_interval_ms, @default_renderer_check_interval_ms)

    unless is_nil(renderer_check_interval_ms) or
             (is_integer(renderer_check_interval_ms) and renderer_check_interval_ms >= 0) do
      raise ArgumentError,
            "viewport renderer_check_interval_ms must be nil or a non-negative integer, got: #{inspect(renderer_check_interval_ms)}"
    end

    %{
      skia_opts: skia_opts,
      renderer_module: renderer_module,
      renderer_opts: renderer_opts,
      input_mask: input_mask,
      renderer_check_interval_ms: renderer_check_interval_ms
    }
  end

  defp ensure_skia_otp_app!(skia_opts, module) when is_list(skia_opts) and is_atom(module) do
    case Keyword.fetch(skia_opts, :otp_app) do
      {:ok, otp_app} when is_atom(otp_app) ->
        skia_opts

      {:ok, other} ->
        raise ArgumentError,
              "mount/1 otp_app must be an atom, got: #{inspect(other)}"

      :error ->
        Keyword.put(skia_opts, :otp_app, infer_otp_app!(module))
    end
  end

  defp infer_otp_app!(module) when is_atom(module) do
    case Application.get_application(module) || infer_otp_app_from_module_root(module) do
      otp_app when is_atom(otp_app) ->
        otp_app

      nil ->
        raise ArgumentError,
              "mount/1 could not infer otp_app for #{inspect(module)}; pass otp_app: :my_app or emerge_skia: [otp_app: :my_app]"
    end
  end

  defp infer_otp_app_from_module_root(module) when is_atom(module) do
    module
    |> Module.split()
    |> List.first()
    |> case do
      nil ->
        nil

      root ->
        root = Macro.underscore(root)

        Enum.find_value(Application.loaded_applications(), fn {otp_app, _description, _version} ->
          if Atom.to_string(otp_app) == root, do: otp_app
        end)
    end
  end
end
