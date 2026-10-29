defmodule Emerge.Runtime.Viewport do
  @moduledoc """
  Runtime for viewports defined with `use Emerge`.

  Application mount arguments remain keyword lists. The configuration returned
  by `mount/1` is an `Emerge.Config` struct, not renderer keywords:

      def mount(opts) do
        output = %Emerge.Config.Window{title: Keyword.get(opts, :title, "My App")}
        {:ok, %Emerge.Config{output: output, input: [events: [:key]]}}
      end

  Implement neither or exactly one of `config/1` and `config/2`. They receive
  the previous normalized desired config; `config/2` also receives application
  state. The first call receives mount config, before renderer creation. Later
  calls run during coalesced rerenders, even if the tree has not changed.
  Callback/validation failures retain the previous valid config. Operational
  renderer failures retain the new desired config for the next callback and
  recovery; the complete live snapshot is retried on the next rerender. Accepted
  config is not proof of hardware application or presentation completion.

      def config(%Emerge.Config{output: %Emerge.Config.Window{} = output} = previous, state) do
        output = %Emerge.Config.Window{output | title: state.title}
        %Emerge.Config{previous | output: output}
      end

  Nested keyword shorthand expands from record defaults, not previous values.
  Use typed struct updates to preserve untouched fields. See `Emerge.Config`
  for groups, output variants, defaults and current implementation limits.

  Raw input observation is opt-in through `Emerge.Config.Input.events`.
  `Emerge.Input` documents every callback payload. This filter does not disable
  element interactions or window-close notifications. `Emerge.UI.Event` documents
  element-local mouse positions and opt-in captured dragging; these use ordinary
  `handle_info/2` messages and the viewport's `c:Emerge.wrap_payload/3` callback.

  Adapter selection and liveness policy live in `Emerge.Config.Runtime`.
  Native ownership is managed internally, not configured by applications.
  See the [viewport configuration documentation](Emerge.html#module-viewport-configuration),
  [input guide](handle_input.html), [0.6 pointer migration guide](0-6.html) and
  [0.5 configuration migration guide](0-5.html).

  Initially-off DRM viewports still acquire and bind output ownership without
  rendering/uploading. Off preserves application messages, config reconciliation,
  latest tree/intent and ownership, while suspending native UI input and destroying
  the presenter. On recreates resources; no GPU state is restored. Power/brightness
  controls require explicit OpenGL, Vulkan or raster (CPU or GPU-upload
  presentation). GPU upload requires an OpenGL-enabled DRM build; automatic API
  power controls remain unsupported. Invalid live/startup controls report errors.
  Software support does not qualify physical panel behavior.
  """

  use GenServer

  require Logger

  alias Emerge.Runtime.VideoEndpoints
  alias Emerge.Runtime.Viewport.Config
  alias Emerge.Runtime.Viewport.Lifecycle
  alias Emerge.Runtime.Viewport.LifecycleSupervisor
  alias Emerge.Runtime.Viewport.ReloadGroup
  alias Emerge.Runtime.Viewport.Renderer
  alias Emerge.Runtime.Viewport.Renderer.Skia
  alias Emerge.Runtime.Viewport.State

  @genserver_start_options [:name, :timeout, :debug, :spawn_opt, :hibernate_after]
  @runtime_key :__emerge__
  @renderer_heartbeat_timeout_ms 1_000

  @type t :: map()

  @impl true
  def init({module, opts}) when is_atom(module) and is_list(opts) do
    init_state(module, opts)
  end

  @impl true
  def handle_continue({:emerge_viewport_mount, opts}, state) do
    handle_continue_mount(opts, state)
  end

  @impl true
  def handle_info({:emerge_viewport_renderer, generation, message}, state) do
    runtime = runtime!(state)

    retiring_close =
      match?({:emerge_skia_close, _}, message) and
        runtime.retiring_renderer_generation == generation

    if runtime.renderer_generation == generation or retiring_close do
      handle_info(message, state)
    else
      {:noreply, state}
    end
  end

  def handle_info(
        {:emerge_viewport, :renderer_ready, owner, generation, renderer, relay, startup_config},
        state
      ) do
    runtime = runtime!(state)

    # A synchronous acquire can have already adopted the session while its ready
    # notification was queued. Never overwrite an active session with that message.
    if runtime.lifecycle == owner and is_nil(runtime.renderer) do
      state =
        update_runtime(
          state,
          &%{
            &1
            | renderer: renderer,
              renderer_config: startup_config,
              config_error: nil,
              config_pending: nil,
              config_applied: nil,
              diff_state: nil,
              renderer_generation: generation,
              retiring_renderer_generation: nil,
              renderer_relay: relay,
              renderer_bound?: false
          }
        )

      {:noreply, render_frame(state, :renderer_recovery)}
    else
      {:noreply, state}
    end
  end

  def handle_info({:emerge_viewport, :renderer_unavailable, owner, generation, result}, state) do
    runtime = runtime!(state)

    if runtime.lifecycle == owner and runtime.renderer_generation == generation do
      if result not in [:ok, :pending],
        do: Logger.error("renderer cleanup failed: #{inspect(result)}")

      {:noreply,
       update_runtime(
         state,
         &%{
           &1
           | renderer: nil,
             renderer_config: nil,
             config_error: nil,
             config_pending: nil,
             config_applied: nil,
             diff_state: nil,
             renderer_generation: nil,
             retiring_renderer_generation: generation,
             renderer_relay: nil,
             renderer_bound?: false
         }
       )}
    else
      {:noreply, state}
    end
  end

  def handle_info({:emerge_viewport, :renderer_error, owner, reason}, state) do
    if runtime!(state).lifecycle == owner,
      do: Logger.warning("renderer restart failed: #{inspect(reason)}")

    {:noreply, state}
  end

  def handle_info({:emerge_skia_config, token, result}, state) do
    runtime = runtime!(state)

    if runtime.config_pending === token and not is_nil(token) do
      state = update_runtime(state, &%{&1 | config_pending: nil})

      state =
        case result do
          :ok ->
            update_runtime(state, fn current ->
              %{
                current
                | config_applied: token,
                  config_error:
                    if(current.renderer_config == current.config,
                      do: nil,
                      else: current.config_error
                    )
              }
            end)

          {:error, reason} when runtime.renderer_config == runtime.config ->
            config_failed(state, reason)

          other when runtime.renderer_config == runtime.config ->
            config_failed(state, {:invalid_config_completion, other})

          _ ->
            state
        end

      {:noreply, state}
    else
      {:noreply, state}
    end
  end

  @impl true
  def handle_info({:emerge_skia_log, level, source, message}, state) do
    state = note_renderer_heartbeat(state)
    log_native_renderer_message(level, source, message)
    {:noreply, state}
  end

  @impl true
  def handle_info({:emerge_skia_close, reason}, state) do
    state = note_renderer_heartbeat(state)
    Logger.info("Emerge viewport window closed for #{inspect(runtime!(state).module)}")

    maybe_log_close_signal(state, fn ->
      "close: viewport received native close reason=#{inspect(reason)}"
    end)

    handle_close_requested(reason, state)
  end

  @impl true
  def handle_info({:emerge_skia_event, event}, state) do
    state = note_renderer_heartbeat(state)
    handle_skia_event(event, state)
  end

  @impl true
  def handle_info({:emerge_viewport, :check_renderer}, state) do
    handle_check_renderer(state)
  end

  @impl true
  def handle_info({:EXIT, pid, :normal}, state) do
    if runtime!(state).lifecycle_supervisor == pid,
      do: {:stop, :normal, state},
      else: {:noreply, state}
  end

  @impl true
  def handle_info({:EXIT, _pid, reason}, state) do
    {:stop, reason, state}
  end

  @impl true
  def handle_info({:emerge_viewport, :source_reloaded, meta}, state) do
    handle_source_reloaded(meta, state)
  end

  @impl true
  def handle_info(message, state) do
    if message == Renderer.heartbeat_message() do
      {:noreply, note_renderer_heartbeat(state)}
    else
      delegate_handle_info(message, state)
    end
  end

  @impl true
  def handle_cast({:emerge_viewport, :flush}, state) do
    handle_flush(state)
  end

  @impl true
  def handle_call({:emerge_viewport, :renderer}, _from, state) do
    {:reply, runtime!(state).renderer, state}
  end

  @impl true
  def terminate(reason, state) do
    terminate_viewport(reason, state)
  end

  @doc false
  @spec start_link(module(), keyword()) :: GenServer.on_start()
  def start_link(module, opts) when is_atom(module) and is_list(opts) do
    genserver_opts = Keyword.take(opts, @genserver_start_options)
    init_opts = Keyword.drop(opts, @genserver_start_options)
    GenServer.start_link(__MODULE__, {module, init_opts}, genserver_opts)
  end

  @doc false
  @spec child_spec(module(), keyword()) :: map()
  def child_spec(module, opts) when is_atom(module) and is_list(opts) do
    %{
      id: Keyword.get(opts, :name, module),
      start: {module, :start_link, [opts]},
      restart: :transient,
      shutdown: :infinity,
      type: :worker
    }
  end

  @doc false
  @spec init_state(module(), keyword()) ::
          {:ok, t(), {:continue, {:emerge_viewport_mount, keyword()}}}
  def init_state(module, opts) when is_atom(module) and is_list(opts) do
    Process.flag(:trap_exit, true)
    {:ok, put_runtime(%{}, %State{module: module}), {:continue, {:emerge_viewport_mount, opts}}}
  end

  @doc false
  @spec handle_continue_mount(keyword(), t()) :: {:noreply, t()} | {:stop, term(), t()}
  def handle_continue_mount(opts, state) when is_list(opts) and is_map(state) do
    runtime = runtime!(state)

    case apply(runtime.module, :mount, [opts]) do
      {:ok, %Emerge.Config{} = mount_opts} ->
        state = mount_viewport(%{}, runtime, mount_opts)
        {:noreply, state}

      {:ok, mounted_state, %Emerge.Config{} = mount_opts} ->
        mounted_state = validate_mount_state!(runtime.module, mounted_state)
        state = mount_viewport(mounted_state, runtime, mount_opts)

        {:noreply, state}

      {:stop, reason} ->
        {:stop, reason, state}

      other ->
        raise ArgumentError,
              "#{inspect(runtime.module)}.mount/1 must return {:ok, %Emerge.Config{}}, {:ok, state, %Emerge.Config{}}, or {:stop, reason}, got: #{inspect(other)}"
    end
  end

  @doc false
  @spec handle_skia_event(term(), t()) :: {:noreply, t()} | {:stop, term(), t()}
  def handle_skia_event(event, state) when is_map(state) do
    case event do
      {id_bin, event_ref} when is_binary(id_bin) ->
        route_element_event(state, id_bin, event_ref, :no_payload)
        {:noreply, state}

      {id_bin, :key_down, route} when is_binary(id_bin) and is_binary(route) ->
        route_element_event(state, id_bin, {:key_down, route}, :no_payload)
        {:noreply, state}

      {id_bin, :key_up, route} when is_binary(id_bin) and is_binary(route) ->
        route_element_event(state, id_bin, {:key_up, route}, :no_payload)
        {:noreply, state}

      {id_bin, :key_press, route} when is_binary(id_bin) and is_binary(route) ->
        route_element_event(state, id_bin, {:key_press, route}, :no_payload)
        {:noreply, state}

      {id_bin, event_type, payload} when is_binary(id_bin) and is_atom(event_type) ->
        route_element_event(state, id_bin, event_type, {:with_payload, payload})
        {:noreply, state}

      _ ->
        state
        |> runtime!()
        |> then(&apply(&1.module, :handle_input, [Emerge.Input.from_native(event), state]))
        |> apply_callback_result(state, :handle_input)
    end
  end

  @doc false
  @spec handle_check_renderer(t()) :: {:noreply, t()} | {:stop, term(), t()}
  def handle_check_renderer(state) when is_map(state) do
    runtime = runtime!(state)

    cond do
      runtime.renderer_module == Skia and is_pid(runtime.lifecycle) ->
        Lifecycle.check(runtime.lifecycle)
        {:noreply, state}

      is_nil(runtime.renderer) ->
        {:noreply, state}

      renderer_heartbeat_recent?(runtime) ->
        {:noreply, maybe_schedule_renderer_check(state)}

      renderer_running?(runtime) ->
        {:noreply, state |> note_renderer_heartbeat() |> maybe_schedule_renderer_check()}

      true ->
        {:stop, :normal, state}
    end
  end

  @doc false
  @spec handle_source_reloaded(term(), t()) :: {:noreply, t()}
  def handle_source_reloaded(_meta, state) when is_map(state) do
    {:noreply, rerender(state)}
  end

  @doc false
  @spec handle_flush(t()) :: {:noreply, t()}
  def handle_flush(state) when is_map(state) do
    state = update_runtime(state, &%{&1 | flush_scheduled?: false})

    if runtime!(state).dirty? do
      {:noreply, render_frame(state, :rerender)}
    else
      {:noreply, state}
    end
  end

  @doc false
  @spec handle_close_requested(term(), t()) :: {:noreply, t()} | {:stop, term(), t()}
  def handle_close_requested(reason, state) when is_map(state) do
    maybe_log_close_signal(state, fn ->
      "close: handle_close begin module=#{inspect(runtime!(state).module)} reason=#{inspect(reason)}"
    end)

    result =
      state
      |> runtime!()
      |> then(&apply(&1.module, :handle_close, [reason, state]))

    maybe_log_close_signal(state, fn ->
      "close: handle_close returned #{inspect(result)}"
    end)

    apply_callback_result(result, state, :handle_close)
  end

  @doc false
  @spec terminate_viewport(term(), t()) :: :ok
  def terminate_viewport(_reason, state) when is_map(state) do
    _ = VideoEndpoints.unregister(self())
    _ = ReloadGroup.leave(self())

    case Map.get(state, @runtime_key) do
      %State{lifecycle: lifecycle, lifecycle_supervisor: supervisor} when is_pid(lifecycle) ->
        _ = safe_invoke(fn -> Lifecycle.close(lifecycle) end)
        _ = safe_invoke(fn -> Supervisor.stop(supervisor, :normal, 7_000) end)
        :ok

      %State{renderer: renderer} = runtime when not is_nil(renderer) ->
        _ = safe_stop_renderer(runtime.renderer_module, renderer)
        :ok

      _ ->
        :ok
    end
  end

  @spec notify_source_reloaded(term()) :: :ok
  def notify_source_reloaded(meta \\ %{}) do
    ReloadGroup.broadcast({:emerge_viewport, :source_reloaded, meta})
  end

  @spec renderer(pid()) :: term()
  def renderer(pid) when is_pid(pid) do
    GenServer.call(pid, {:emerge_viewport, :renderer})
  end

  @spec rerender(map()) :: map()
  def rerender(state) when is_map(state) do
    runtime = %{runtime!(state) | dirty?: true}
    state = put_runtime(state, runtime)

    if runtime.flush_scheduled? do
      state
    else
      GenServer.cast(self(), {:emerge_viewport, :flush})
      put_runtime(state, %{runtime | flush_scheduled?: true})
    end
  end

  @spec default_wrap_payload(term(), term(), term()) :: term()
  def default_wrap_payload(message, payload, _event_type) when is_tuple(message) do
    Tuple.insert_at(message, tuple_size(message), payload)
  end

  def default_wrap_payload(message, payload, _event_type), do: {message, payload}

  @doc false
  @spec runtime!(map()) :: term()
  def runtime!(state) when is_map(state) do
    case Map.fetch(state, @runtime_key) do
      {:ok, %State{} = runtime} ->
        runtime

      {:ok, other} ->
        raise ArgumentError,
              "viewport state reserves #{inspect(@runtime_key)} for #{inspect(State)}, got: #{inspect(other)}"

      :error ->
        raise ArgumentError,
              "viewport state is missing reserved key #{inspect(@runtime_key)}"
    end
  end

  defp put_runtime(state, %State{} = runtime) when is_map(state) do
    Map.put(state, @runtime_key, runtime)
  end

  defp update_runtime(state, fun) when is_map(state) and is_function(fun, 1) do
    put_runtime(state, fun.(runtime!(state)))
  end

  defp validate_mount_state!(module, state) when is_atom(module) do
    state = expect_state_map!(module, :mount, state)

    if Map.has_key?(state, @runtime_key) do
      raise ArgumentError,
            "#{inspect(module)}.mount/1 state must not contain reserved key #{inspect(@runtime_key)}"
    end

    state
  end

  defp validate_callback_state!(module, callback_name, state)
       when is_atom(module) and is_atom(callback_name) do
    state = expect_state_map!(module, callback_name, state)

    _ = runtime!(state)
    state
  end

  defp route_element_event(state, id_bin, event_ref, payload_mode) do
    runtime = runtime!(state)

    if is_nil(runtime.diff_state) do
      :ok
    else
      case Emerge.Engine.lookup_event(runtime.diff_state, id_bin, event_ref) do
        {:ok, {pid, message}} when is_pid(pid) ->
          routed_message =
            case payload_mode do
              :no_payload ->
                message

              {:with_payload, payload} ->
                apply(runtime.module, :wrap_payload, [message, payload, event_ref])
            end

          send(pid, routed_message)

        _ ->
          :ok
      end
    end
  end

  defp delegate_handle_info(message, state) do
    state
    |> runtime!()
    |> then(&apply(&1.module, :handle_info, [message, state]))
    |> apply_callback_result(state, :handle_info)
  end

  defp apply_callback_result({:noreply, next_state}, state, callback_name) do
    module = runtime!(state).module
    {:noreply, validate_callback_state!(module, callback_name, next_state)}
  end

  defp apply_callback_result({:stop, reason, next_state}, state, callback_name) do
    module = runtime!(state).module
    {:stop, reason, validate_callback_state!(module, callback_name, next_state)}
  end

  defp apply_callback_result(other, state, callback_name) do
    module = runtime!(state).module

    raise ArgumentError,
          "#{inspect(module)}.#{callback_name}/2 must return {:noreply, state} or {:stop, reason, state}, got: #{inspect(other)}"
  end

  defp maybe_schedule_renderer_check(state) when is_map(state) do
    runtime = runtime!(state)

    if runtime.renderer_module != Skia and is_integer(runtime.renderer_check_interval_ms) and
         runtime.renderer_check_interval_ms > 0 do
      Process.send_after(
        self(),
        {:emerge_viewport, :check_renderer},
        runtime.renderer_check_interval_ms
      )
    end

    state
  end

  defp note_renderer_heartbeat(state) when is_map(state) do
    if Map.has_key?(state, @runtime_key) do
      update_runtime(state, fn runtime ->
        %{runtime | last_renderer_heartbeat_at_ms: monotonic_ms()}
      end)
    else
      state
    end
  end

  defp renderer_heartbeat_recent?(runtime) do
    case runtime.last_renderer_heartbeat_at_ms do
      last_seen_ms when is_integer(last_seen_ms) ->
        monotonic_ms() - last_seen_ms <= @renderer_heartbeat_timeout_ms

      _ ->
        false
    end
  end

  defp renderer_running?(%State{renderer_module: renderer_module, renderer: renderer}) do
    case safe_invoke(fn -> renderer_module.running?(renderer) end) do
      {:ok, true} -> true
      _ -> false
    end
  end

  defp monotonic_ms do
    System.monotonic_time(:millisecond)
  end

  defp put_mount_config(state, %Config{} = mount_config) when is_map(state) do
    update_runtime(state, fn runtime ->
      %{
        runtime
        | config: mount_config.config,
          renderer_module: mount_config.renderer_module,
          renderer_opts: mount_config.renderer_opts,
          skia_opts: mount_config.skia_opts,
          input_mask: mount_config.input_mask,
          renderer_check_interval_ms: mount_config.renderer_check_interval_ms
      }
    end)
  end

  defp render_frame(state, phase) when is_map(state) do
    case safe_invoke(fn -> reconcile_config(state, phase) end) do
      {:ok, next_state} ->
        cond do
          runtime!(next_state).config_error != nil ->
            clear_pending_rerender(next_state)

          Emerge.Config.Changes.display_off?(runtime!(next_state).config) ->
            acquire_off_renderer(next_state, phase)

          true ->
            render_tree_frame(next_state, phase)
        end

      {:error, failure} ->
        log_render_failure(runtime!(state).module, phase, failure)
        clear_pending_rerender(state)
    end
  end

  defp reconcile_config(state, phase) do
    runtime = runtime!(state)

    desired =
      case config_callback_arity!(runtime.module) do
        nil -> runtime.config
        1 -> apply(runtime.module, :config, [runtime.config])
        2 -> apply(runtime.module, :config, [runtime.config, state])
      end

    parsed = Config.parse!(runtime.module, desired)

    state =
      if parsed.config == runtime.config do
        state
      else
        if phase != :initial_render do
          Emerge.Config.Changes.validate!(runtime.config, parsed.config)

          if runtime.renderer_module == Skia do
            case EmergeSkia.validate_config_change(runtime.config, parsed.config) do
              :ok -> :ok
              {:error, reason} -> raise ArgumentError, "unsupported config: #{inspect(reason)}"
            end
          end
        end

        # Recovery and the next callback must see validated desired intent even
        # when a backend cannot apply it right now.
        :ok = Lifecycle.configure(runtime.lifecycle, parsed)
        put_mount_config(state, parsed)
      end

    reconcile_renderer_config(state)
  end

  defp reconcile_renderer_config(state) do
    runtime = runtime!(state)

    if runtime.renderer &&
         (runtime.renderer_config != runtime.config or runtime.config_error != nil) do
      result =
        safe_invoke(fn ->
          runtime.renderer_module.configure(
            runtime.renderer,
            runtime.renderer_config,
            runtime.config
          )
        end)

      case result do
        {:ok, :ok} ->
          update_runtime(
            state,
            &%{
              &1
              | renderer_config: &1.config,
                config_error: nil,
                config_pending: nil
            }
          )

        {:ok, {:pending, {session, revision} = token}}
        when is_integer(session) and session >= 0 and is_integer(revision) and revision > 0 ->
          update_runtime(
            state,
            &%{
              &1
              | renderer_config: &1.config,
                config_error: nil,
                config_pending: token
            }
          )

        {:ok, {:error, reason}} ->
          config_failed(state, reason)

        {:error, failure} ->
          config_failed(state, failure)

        {:ok, other} ->
          config_failed(state, {:invalid_config_result, other})
      end
    else
      state
    end
  end

  defp config_failed(state, reason) do
    log_render_failure(runtime!(state).module, :config, inspect(reason))
    update_runtime(state, &%{&1 | config_error: {:error, reason}})
  end

  defp config_callback_arity!(module) do
    case {function_exported?(module, :config, 1), function_exported?(module, :config, 2)} do
      {false, false} ->
        nil

      {true, false} ->
        1

      {false, true} ->
        2

      {true, true} ->
        raise ArgumentError, "#{inspect(module)} must define at most one of config/1 or config/2"
    end
  end

  defp render_tree_frame(state, phase) do
    case safe_render_tree(state) do
      {:ok, tree} ->
        case apply_tree_to_renderer(state, tree) do
          {:ok, next_state} ->
            clear_pending_rerender(next_state)

          {:error, failure} ->
            log_render_failure(runtime!(state).module, phase, failure)
            clear_pending_rerender(state)
        end

      {:error, failure} ->
        log_render_failure(runtime!(state).module, phase, failure)
        clear_pending_rerender(state)
    end
  end

  defp apply_tree_to_renderer(state, tree) when is_map(state) do
    runtime = runtime!(state)

    if not is_nil(runtime.renderer) and not is_nil(runtime.diff_state) do
      patch_existing_renderer(state, runtime, tree)
    else
      start_and_upload_renderer(state, tree)
    end
  end

  defp patch_existing_renderer(state, runtime, tree) do
    patch_fun =
      if function_exported?(runtime.renderer_module, :patch_tree_runtime, 3) do
        :patch_tree_runtime
      else
        :patch_tree
      end

    case safe_invoke(fn ->
           apply(runtime.renderer_module, patch_fun, [runtime.renderer, runtime.diff_state, tree])
         end) do
      {:ok, {diff_state, _assigned}} ->
        {:ok, update_runtime(state, &%{&1 | diff_state: diff_state})}

      {:ok, other} ->
        {:error, "renderer patch failed with unexpected result: #{inspect(other)}"}

      {:error, failure} ->
        {:error, failure}
    end
  end

  # Off defers render/serialization, not ownership. A native owner is still
  # necessary to suppress inherited scanout and keep the output reserved.
  defp acquire_off_renderer(state, phase) do
    result =
      if runtime!(state).renderer_bound?,
        do: {:ok, state},
        else: acquire_renderer(state)

    case result do
      {:ok, state} ->
        clear_pending_rerender(state)

      {:error, failure} ->
        log_render_failure(runtime!(state).module, phase, failure)
        clear_pending_rerender(state)
    end
  end

  defp start_and_upload_renderer(state, tree) do
    with {:ok, state} <- acquire_renderer(state) do
      runtime = runtime!(state)

      if runtime.config_error do
        {:ok, state}
      else
        case upload_initial_tree(state, runtime.renderer, tree) do
          {:ok, next_state} ->
            {:ok, next_state}

          {:error, failure} ->
            _ = safe_stop_renderer(runtime.renderer_module, runtime.renderer)
            {:error, failure}
        end
      end
    end
  end

  defp acquire_renderer(state) do
    runtime = runtime!(state)

    case safe_invoke(fn ->
           Lifecycle.acquire(runtime.lifecycle)
         end) do
      {:ok, {:ok, renderer, generation, relay, startup_config}} ->
        state =
          state
          |> update_runtime(
            &%{
              &1
              | renderer: renderer,
                renderer_config:
                  if(runtime.renderer_generation == generation,
                    do: runtime.renderer_config,
                    else: startup_config
                  ),
                config_pending:
                  if(runtime.renderer_generation == generation, do: runtime.config_pending),
                config_applied:
                  if(runtime.renderer_generation == generation, do: runtime.config_applied),
                diff_state: if(runtime.renderer_generation == generation, do: runtime.diff_state),
                renderer_generation: generation,
                retiring_renderer_generation: nil,
                renderer_relay: relay,
                renderer_bound?:
                  runtime.renderer_generation == generation and runtime.renderer_bound?
            }
          )
          |> reconcile_renderer_config()

        if runtime!(state).config_error != nil or runtime!(state).renderer_bound? do
          {:ok, state}
        else
          bind_renderer(state)
        end

      {:ok, {:error, reason}} ->
        {:error, "renderer start failed: #{inspect(reason)}"}

      {:ok, other} ->
        {:error, "renderer start failed with unexpected result: #{inspect(other)}"}

      {:error, failure} ->
        {:error, failure}
    end
  end

  defp bind_renderer(state) do
    runtime = runtime!(state)
    renderer = runtime.renderer

    case safe_invoke(fn ->
           target = runtime.renderer_relay || self()
           :ok = runtime.renderer_module.set_input_target(renderer, target)
           :ok = runtime.renderer_module.set_log_target(renderer, target)

           if is_integer(runtime.input_mask) do
             :ok = runtime.renderer_module.set_input_mask(renderer, runtime.input_mask)
           end

           :ok
         end) do
      {:ok, :ok} ->
        {:ok,
         state
         |> update_runtime(
           &%{&1 | renderer_bound?: true, last_renderer_heartbeat_at_ms: monotonic_ms()}
         )
         |> maybe_schedule_renderer_check()}

      {:error, failure} ->
        _ = safe_stop_renderer(runtime.renderer_module, renderer)
        {:error, failure}
    end
  end

  defp upload_initial_tree(state, renderer, tree) when is_map(state) do
    runtime = runtime!(state)

    case safe_invoke(fn -> runtime.renderer_module.upload_tree(renderer, tree) end) do
      {:ok, {diff_state, _assigned}} ->
        state =
          state
          |> update_runtime(fn current ->
            %{
              current
              | renderer: renderer,
                diff_state: diff_state,
                last_renderer_heartbeat_at_ms: monotonic_ms()
            }
          end)

        :ok = VideoEndpoints.register(self(), renderer)
        {:ok, state}

      {:ok, other} ->
        {:error, "renderer upload failed with unexpected result: #{inspect(other)}"}

      {:error, failure} ->
        {:error, failure}
    end
  end

  defp register_reload_viewport(state) when is_map(state) do
    :ok = ReloadGroup.join(self())
    state
  end

  defp clear_pending_rerender(state) when is_map(state) do
    update_runtime(state, &%{&1 | dirty?: false, flush_scheduled?: false})
  end

  defp safe_invoke(fun) when is_function(fun, 0) do
    {:ok, fun.()}
  rescue
    exception ->
      {:error, Exception.format(:error, exception, __STACKTRACE__)}
  catch
    kind, reason ->
      {:error, Exception.format(kind, reason, __STACKTRACE__)}
  end

  defp safe_render_tree(state) when is_map(state) do
    runtime = runtime!(state)
    safe_invoke(fn -> invoke_render(runtime.module, state) end)
  end

  defp mount_viewport(mounted_state, runtime, mount_opts)
       when is_map(mounted_state) and is_struct(runtime, State) and
              is_struct(mount_opts, Emerge.Config) do
    _ = validate_render_callback_shape!(runtime.module)
    _ = config_callback_arity!(runtime.module)
    mount_config = Config.parse!(runtime.module, mount_opts)
    {:ok, supervisor} = LifecycleSupervisor.start_link(self(), mount_config)
    lifecycle = LifecycleSupervisor.lifecycle(supervisor)

    mounted_state
    |> put_runtime(runtime)
    |> put_mount_config(mount_config)
    |> update_runtime(&%{&1 | lifecycle: lifecycle, lifecycle_supervisor: supervisor})
    |> register_reload_viewport()
    |> render_frame(:initial_render)
  end

  defp invoke_render(module, state) when is_atom(module) and is_map(state) do
    case validate_render_callback_shape!(module) do
      0 -> apply(module, :render, [])
      1 -> apply(module, :render, [state])
    end
  end

  defp validate_render_callback_shape!(module) when is_atom(module) do
    _ = Code.ensure_loaded(module)

    render0? = function_exported?(module, :render, 0)
    render1? = function_exported?(module, :render, 1)

    case {render0?, render1?} do
      {true, false} ->
        0

      {false, true} ->
        1

      {true, true} ->
        raise ArgumentError,
              "#{inspect(module)} must define exactly one of render/0 or render/1, but defines both"

      {false, false} ->
        raise ArgumentError,
              "#{inspect(module)} must define exactly one of render/0 or render/1"
    end
  end

  defp log_render_failure(module, phase, failure) when is_atom(module) do
    Logger.error([
      "Emerge viewport ",
      phase_label(phase),
      " failed for ",
      inspect(module),
      ":\n",
      failure
    ])
  end

  defp log_native_renderer_message(level, source, message) do
    Logger.log(normalize_native_log_level(level, source), fn ->
      {[
         "EmergeSkia native[",
         to_string(source),
         "] ",
         to_string(message)
       ], [native_renderer: true, native_renderer_source: source]}
    end)
  end

  # Event-runtime traces are high-volume diagnostics, not application lifecycle information.
  # Keep older native artifacts that labeled them as :info quiet at Logger's default level too.
  defp normalize_native_log_level(level, source)
       when level in [:debug, :info] and source in ["event_runtime", :event_runtime],
       do: :debug

  defp normalize_native_log_level(level, _source)
       when level in [:debug, :info, :warning, :error],
       do: level

  defp normalize_native_log_level(_level, _source), do: :info

  defp phase_label(:config), do: "config reconciliation"
  defp phase_label(:initial_render), do: "initial render"
  defp phase_label(:rerender), do: "rerender"
  defp phase_label(:renderer_recovery), do: "renderer recovery"

  defp safe_stop_renderer(renderer_module, renderer) do
    renderer_module.stop(renderer)
  catch
    _kind, _reason -> :ok
  end

  defp maybe_log_close_signal(state, message_fun)
       when is_map(state) and is_function(message_fun, 0) do
    runtime = runtime!(state)

    if Keyword.get(runtime.skia_opts, :close_signal_log, false) do
      Logger.info(message_fun.())
    end

    :ok
  end

  defp expect_state_map!(module, :mount, state) when is_atom(module) do
    unless is_map(state) do
      raise ArgumentError,
            "#{inspect(module)}.mount/1 must return {:ok, state, opts} with state as a map, got: #{inspect(state)}"
    end

    state
  end

  defp expect_state_map!(module, callback_name, state)
       when is_atom(module) and is_atom(callback_name) do
    unless is_map(state) do
      raise ArgumentError,
            "#{inspect(module)}.#{callback_name}/2 must return a state map, got: #{inspect(state)}"
    end

    state
  end
end
