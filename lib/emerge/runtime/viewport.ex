defmodule Emerge.Runtime.Viewport do
  @moduledoc """
  Runtime GenServer backing `use Emerge` viewport modules.

  This module owns the process lifecycle, renderer integration, tree upload and
  patch flow, and event routing used by the public `Emerge` API.

  Most application code should use `Emerge` directly instead of calling this
  module.
  """

  use GenServer

  require Logger

  alias Emerge.Runtime.Viewport.Config
  alias Emerge.Runtime.Viewport.ReloadGroup
  alias Emerge.Runtime.Viewport.State

  @genserver_start_options [:name, :timeout, :debug, :spawn_opt, :hibernate_after]
  @runtime_key :__emerge__

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
  def handle_info({:emerge_skia_log, level, source, message}, state) do
    log_native_renderer_message(level, source, message)
    {:noreply, state}
  end

  @impl true
  def handle_info({:emerge_skia_event, event}, state) do
    handle_skia_event(event, state)
  end

  @impl true
  def handle_info({:emerge_viewport, :check_renderer}, state) do
    handle_check_renderer(state)
  end

  @impl true
  def handle_info({:emerge_viewport, :source_reloaded, meta}, state) do
    handle_source_reloaded(meta, state)
  end

  @impl true
  def handle_info(message, state) do
    delegate_handle_info(message, state)
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
      type: :worker
    }
  end

  @doc false
  @spec init_state(module(), keyword()) ::
          {:ok, t(), {:continue, {:emerge_viewport_mount, keyword()}}}
  def init_state(module, opts) when is_atom(module) and is_list(opts) do
    {:ok, put_runtime(%{}, %State{module: module}), {:continue, {:emerge_viewport_mount, opts}}}
  end

  @doc false
  @spec handle_continue_mount(keyword(), t()) :: {:noreply, t()} | {:stop, term(), t()}
  def handle_continue_mount(opts, state) when is_list(opts) and is_map(state) do
    runtime = runtime!(state)

    case apply(runtime.module, :mount, [opts]) do
      {:ok, mounted_state, mount_opts} when is_list(mount_opts) ->
        validate_mount_state!(runtime.module, mounted_state)
        mount_config = Config.parse!(runtime.module, mount_opts)

        state =
          mounted_state
          |> put_runtime(runtime)
          |> put_mount_config(mount_config)
          |> register_reload_viewport()
          |> render_frame(:initial_render)

        {:noreply, state}

      {:stop, reason} ->
        {:stop, reason, state}

      other ->
        raise ArgumentError,
              "#{inspect(runtime.module)}.mount/1 must return {:ok, state, opts} or {:stop, reason}, got: #{inspect(other)}"
    end
  end

  @doc false
  @spec handle_skia_event(term(), t()) :: {:noreply, t()} | {:stop, term(), t()}
  def handle_skia_event(event, state) when is_map(state) do
    case event do
      {id_bin, event_type} when is_binary(id_bin) and is_atom(event_type) ->
        route_element_event(state, id_bin, event_type, :no_payload)
        {:noreply, state}

      {id_bin, event_type, payload} when is_binary(id_bin) and is_atom(event_type) ->
        route_element_event(state, id_bin, event_type, {:with_payload, payload})
        {:noreply, state}

      _ ->
        state
        |> runtime!()
        |> then(&apply(&1.module, :handle_input, [event, state]))
        |> apply_callback_result(state, :handle_input)
    end
  end

  @doc false
  @spec handle_check_renderer(t()) :: {:noreply, t()} | {:stop, term(), t()}
  def handle_check_renderer(state) when is_map(state) do
    runtime = runtime!(state)

    cond do
      is_nil(runtime.renderer) ->
        {:noreply, state}

      runtime.renderer_module.running?(runtime.renderer) ->
        {:noreply, maybe_schedule_renderer_check(state)}

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
  @spec terminate_viewport(term(), t()) :: :ok
  def terminate_viewport(_reason, state) when is_map(state) do
    case Map.get(state, @runtime_key) do
      %State{renderer: nil} ->
        :ok

      %State{} = runtime ->
        _ = ReloadGroup.leave(self())
        _ = safe_stop_renderer(runtime.renderer_module, runtime.renderer)
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

  @spec default_wrap_payload(term(), term(), atom()) :: term()
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

  defp route_element_event(state, id_bin, event_type, payload_mode) do
    runtime = runtime!(state)

    if is_nil(runtime.diff_state) do
      :ok
    else
      case Emerge.Engine.lookup_event(runtime.diff_state, id_bin, event_type) do
        {:ok, {pid, message}} when is_pid(pid) ->
          routed_message =
            case payload_mode do
              :no_payload ->
                message

              {:with_payload, payload} ->
                apply(runtime.module, :wrap_payload, [message, payload, event_type])
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

    if is_integer(runtime.renderer_check_interval_ms) and runtime.renderer_check_interval_ms > 0 do
      Process.send_after(
        self(),
        {:emerge_viewport, :check_renderer},
        runtime.renderer_check_interval_ms
      )
    end

    state
  end

  defp put_mount_config(state, %Config{} = mount_config) when is_map(state) do
    update_runtime(state, fn runtime ->
      %{
        runtime
        | renderer_module: mount_config.renderer_module,
          renderer_opts: mount_config.renderer_opts,
          skia_opts: mount_config.skia_opts,
          input_mask: mount_config.input_mask,
          renderer_check_interval_ms: mount_config.renderer_check_interval_ms
      }
    end)
  end

  defp render_frame(state, phase) when is_map(state) do
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
      start_and_upload_renderer(state, runtime, tree)
    end
  end

  defp patch_existing_renderer(state, runtime, tree) do
    case safe_invoke(fn ->
           runtime.renderer_module.patch_tree(runtime.renderer, runtime.diff_state, tree)
         end) do
      {:ok, {diff_state, _assigned}} ->
        {:ok, update_runtime(state, &%{&1 | diff_state: diff_state})}

      {:ok, other} ->
        {:error, "renderer patch failed with unexpected result: #{inspect(other)}"}

      {:error, failure} ->
        {:error, failure}
    end
  end

  defp start_and_upload_renderer(state, runtime, tree) do
    case safe_invoke(fn ->
           runtime.renderer_module.start(runtime.skia_opts, runtime.renderer_opts)
         end) do
      {:ok, {:ok, renderer}} ->
        case upload_initial_tree(state, renderer, tree) do
          {:ok, next_state} ->
            {:ok, next_state}

          {:error, failure} ->
            _ = safe_stop_renderer(runtime.renderer_module, renderer)
            {:error, failure}
        end

      {:ok, {:error, reason}} ->
        {:error, "renderer start failed: #{inspect(reason)}"}

      {:ok, other} ->
        {:error, "renderer start failed with unexpected result: #{inspect(other)}"}

      {:error, failure} ->
        {:error, failure}
    end
  end

  defp upload_initial_tree(state, renderer, tree) when is_map(state) do
    runtime = runtime!(state)

    case safe_invoke(fn ->
           :ok = runtime.renderer_module.set_input_target(renderer, self())
           :ok = runtime.renderer_module.set_log_target(renderer, self())

           if is_integer(runtime.input_mask) do
             :ok = runtime.renderer_module.set_input_mask(renderer, runtime.input_mask)
           end

           runtime.renderer_module.upload_tree(renderer, tree)
         end) do
      {:ok, {diff_state, _assigned}} ->
        state =
          state
          |> update_runtime(fn current ->
            %{current | renderer: renderer, diff_state: diff_state}
          end)
          |> maybe_schedule_renderer_check()

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
    safe_invoke(fn -> apply(runtime.module, :render, [state]) end)
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
    Logger.log(normalize_native_log_level(level), fn ->
      {[
         "EmergeSkia native[",
         to_string(source),
         "] ",
         to_string(message)
       ], [native_renderer: true, native_renderer_source: source]}
    end)
  end

  defp normalize_native_log_level(level) when level in [:debug, :info, :warning, :error],
    do: level

  defp normalize_native_log_level(_level), do: :info

  defp phase_label(:initial_render), do: "initial render"
  defp phase_label(:rerender), do: "rerender"

  defp safe_stop_renderer(renderer_module, renderer) do
    renderer_module.stop(renderer)
  catch
    _kind, _reason -> :ok
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
