defmodule EmergeSkia.Macos.Session do
  @moduledoc false

  import Bitwise

  alias Emerge.Runtime.Viewport.Renderer, as: ViewportRenderer

  def base_state(input_mask_all) do
    %{
      running: false,
      input_target: nil,
      log_target: nil,
      input_mask: input_mask_all,
      input_ready: false,
      pending_resize: nil,
      pending_focus: nil,
      pending_close: false,
      pending_logs: [],
      pending_element_events: []
    }
  end

  def update_metadata(state, session_id, key, value, input_mask_all) do
    sessions =
      Map.update(
        state.sessions,
        session_id,
        Map.put(base_state(input_mask_all), key, value),
        &Map.put(&1, key, value)
      )

    %{state | sessions: sessions}
  end

  def running?(state, session_id) do
    case Map.fetch(state.sessions, session_id) do
      {:ok, %{running: running?}} -> running?
      :error -> false
    end
  end

  def mark_stopped(state, session_id, input_mask_all) do
    update_session(state, session_id, &Map.put(&1, :running, false), input_mask_all)
  end

  def flush(state, session_id, input_mask_resize, input_mask_focus) do
    case Map.fetch(state.sessions, session_id) do
      {:ok, session} ->
        {state, session} = flush_logs(state, session)
        {state, session} = flush_close(state, session)
        {state, session} = flush_element_events(state, session)
        {state, session} = flush_resize(state, session, input_mask_resize)
        {state, session} = flush_focus(state, session, input_mask_focus)
        put_session(state, session_id, session)

      :error ->
        state
    end
  end

  def buffer_resize(state, session_id, width, height, scale_factor, input_mask_all) do
    update_session(
      state,
      session_id,
      fn session -> %{session | pending_resize: {width, height, scale_factor}} end,
      input_mask_all
    )
  end

  def buffer_focus(state, session_id, focused, input_mask_all) do
    update_session(
      state,
      session_id,
      fn session -> %{session | pending_focus: focused} end,
      input_mask_all
    )
  end

  def buffer_close(state, session_id, input_mask_all) do
    state
    |> mark_stopped(session_id, input_mask_all)
    |> update_session(
      session_id,
      fn session -> %{session | pending_close: true} end,
      input_mask_all
    )
  end

  def buffer_log(state, session_id, {level, source, message}, input_mask_all) do
    update_session(
      state,
      session_id,
      fn session ->
        %{session | pending_logs: session.pending_logs ++ [{level, source, message}]}
      end,
      input_mask_all
    )
  end

  def buffer_element_event(state, session_id, event, input_mask_all) do
    update_session(
      state,
      session_id,
      fn session ->
        %{session | pending_element_events: session.pending_element_events ++ [event]}
      end,
      input_mask_all
    )
  end

  def maybe_dispatch_input(state, session_id, mask_bit, event) do
    case Map.fetch(state.sessions, session_id) do
      {:ok, %{input_target: pid, input_ready: true, input_mask: mask}} when is_pid(pid) ->
        if (mask &&& mask_bit) != 0 do
          send(pid, {:emerge_skia_event, event})
        end

        state

      _ ->
        state
    end
  end

  def maybe_dispatch_running(state, session_id) do
    case Map.fetch(state.sessions, session_id) do
      {:ok, %{input_target: pid}} when is_pid(pid) ->
        send(pid, ViewportRenderer.heartbeat_message())
        state

      _ ->
        state
    end
  end

  defp flush_logs(state, %{log_target: nil} = session), do: {state, session}

  defp flush_logs(state, %{log_target: log_target, pending_logs: logs} = session) do
    Enum.each(logs, fn {level, source, message} ->
      send(log_target, {:emerge_skia_log, level, source, message})
    end)

    {state, %{session | pending_logs: []}}
  end

  defp flush_close(state, %{input_target: nil} = session), do: {state, session}

  defp flush_close(state, %{input_target: input_target, pending_close: true} = session) do
    send(input_target, {:emerge_skia_close, :window_close_requested})
    {state, %{session | pending_close: false}}
  end

  defp flush_close(state, session), do: {state, session}

  defp flush_element_events(state, %{input_target: nil} = session), do: {state, session}

  defp flush_element_events(
         state,
         %{input_target: input_target, pending_element_events: events} = session
       ) do
    Enum.each(events, &send(input_target, {:emerge_skia_event, &1}))
    {state, %{session | pending_element_events: []}}
  end

  defp flush_resize(state, %{input_target: nil} = session, _input_mask_resize),
    do: {state, session}

  defp flush_resize(state, %{input_ready: false} = session, _input_mask_resize),
    do: {state, session}

  defp flush_resize(state, %{pending_resize: nil} = session, _input_mask_resize),
    do: {state, session}

  defp flush_resize(
         state,
         %{
           input_target: input_target,
           input_mask: mask,
           pending_resize: {width, height, scale_factor}
         } =
           session,
         input_mask_resize
       ) do
    if (mask &&& input_mask_resize) != 0 do
      send(input_target, {:emerge_skia_event, {:resized, {width, height, scale_factor}}})
    end

    {state, %{session | pending_resize: nil}}
  end

  defp flush_focus(state, %{input_target: nil} = session, _input_mask_focus), do: {state, session}

  defp flush_focus(state, %{input_ready: false} = session, _input_mask_focus),
    do: {state, session}

  defp flush_focus(state, %{pending_focus: nil} = session, _input_mask_focus),
    do: {state, session}

  defp flush_focus(
         state,
         %{input_target: input_target, input_mask: mask, pending_focus: focused} = session,
         input_mask_focus
       ) do
    if (mask &&& input_mask_focus) != 0 do
      send(input_target, {:emerge_skia_event, {:focused, focused}})
    end

    {state, %{session | pending_focus: nil}}
  end

  defp put_session(state, session_id, session) do
    %{state | sessions: Map.put(state.sessions, session_id, session)}
  end

  defp update_session(state, session_id, fun, input_mask_all) when is_function(fun, 1) do
    sessions =
      Map.update(
        state.sessions,
        session_id,
        fun.(base_state(input_mask_all)),
        fun
      )

    %{state | sessions: sessions}
  end
end
