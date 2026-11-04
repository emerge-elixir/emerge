defmodule Emerge.DisplayWakeTest do
  use ExUnit.Case, async: false
  import ExUnit.CaptureLog
  alias Emerge.Config

  defmodule Adapter do
    @behaviour Emerge.Runtime.Viewport.Renderer
    def start(config, opts),
      do: Agent.start_link(fn -> %{config: config, observer: opts[:observer], revision: 0} end)

    def stop(renderer), do: Agent.stop(renderer)
    def running?(renderer), do: Process.alive?(renderer)
    def set_input_target(_, _), do: :ok
    def set_log_target(_, _), do: :ok
    def set_input_mask(_, _), do: :ok

    def configure(renderer, _, config) do
      revision =
        Agent.get_and_update(renderer, fn s ->
          send(s.observer, {:configured, config})
          {s.revision + 1, %{s | config: config, revision: s.revision + 1}}
        end)

      {:pending, {101, revision}}
    end

    def upload_tree(renderer, tree) do
      Agent.get(renderer, &send(&1.observer, {:uploaded, tree}))
      diff = Emerge.Engine.diff_state_new(tree)
      {diff, diff.tree}
    end

    def patch_tree(renderer, diff, tree) do
      Agent.get(renderer, &send(&1.observer, {:uploaded, tree}))
      {_, next, assigned} = Emerge.Engine.diff_state_update(diff, tree)
      {next, assigned}
    end
  end

  defmodule Screen do
    use Emerge

    def mount(opts) do
      {:ok,
       %{
         observer: opts[:observer],
         value: "initial",
         policy: :double_tap,
         forced: nil,
         events: Keyword.get(opts, :events, [])
       },
       %Config{
         output: %Config.DRM{display: :off, wake_on_touch: :double_tap},
         renderer: [api: :raster],
         runtime: [adapter: Adapter, adapter_opts: opts, health_check_interval_ms: nil]
       }}
    end

    def config(previous, state) do
      send(state.observer, {:previous, previous})

      %{
        previous
        | output: %{
            previous.output
            | display: state.forced || previous.output.display,
              wake_on_touch: state.policy
          },
          input: %{previous.input | events: state.events}
      }
    end

    def render(state) do
      send(state.observer, {:rendered, state.value})
      text(state.value)
    end

    def handle_input(event, state) do
      send(state.observer, {:observed_input, event})
      {:noreply, state}
    end

    def handle_info({:change, attrs}, state),
      do: {:noreply, Viewport.rerender(Map.merge(state, attrs))}

    def handle_info(other, state) do
      send(state.observer, {:unexpected_user_message, other})
      {:noreply, state}
    end
  end

  defmodule StateScreen do
    use Emerge

    def mount(opts) do
      {:ok,
       %{
         observer: opts[:observer],
         sleeping?: true,
         veto?: opts[:veto?] || false,
         fail?: opts[:fail?] || false
       },
       %Config{
         output: %Config.DRM{
           display: :off,
           wake_on_touch: Keyword.get(opts, :wake_on_touch, :long_press)
         },
         input: [events: Keyword.get(opts, :events, [:display_wake, :key])],
         renderer: [api: :raster],
         runtime: [adapter: Adapter, adapter_opts: opts, health_check_interval_ms: nil]
       }}
    end

    def config(previous, state) do
      %{previous | output: %{previous.output | display: if(state.sleeping?, do: :off, else: :on)}}
    end

    def handle_input({:display_wake, gesture}, state) do
      send(state.observer, {:wake_input, gesture})
      if state.fail?, do: raise("wake input failed")
      {:noreply, %{state | sleeping?: state.veto?}}
    end

    def handle_input(event, state) do
      send(state.observer, {:other_input, event})
      {:noreply, state}
    end

    def render(state) do
      send(state.observer, :state_rendered)
      text("awake")
    end
  end

  defp runtime(pid), do: :sys.get_state(pid).__emerge__

  defp wake(pid, gesture, opts \\ []) do
    renderer = Emerge.renderer(pid)

    token =
      Agent.get_and_update(renderer, fn s ->
        revision = s.revision + 1

        {{101, revision},
         %{
           s
           | config: %{s.config | output: %{s.config.output | display: :on}},
             revision: revision
         }}
      end)

    send(pid, {:emerge_skia_display_wake, token, gesture})
    if opts[:complete?], do: send(pid, {:emerge_skia_config, token, :ok})
    _ = :sys.get_state(pid)
    token
  end

  test "unsubscribed wake updates intent and renders current state without invoking input" do
    pid = start_supervised!({Screen, observer: self()})
    renderer = Emerge.renderer(pid)
    assert_receive {:previous, %Config{output: %Config.DRM{display: :off}}}
    refute_receive {:rendered, _}, 20
    send(pid, {:change, %{value: "changed while off"}})
    assert runtime(pid).config.input.events == []
    token = wake(pid, :double_tap, complete?: true)
    assert_receive {:rendered, "changed while off"}
    assert_receive {:uploaded, _}
    current = runtime(pid)
    assert current.config.output.display == :on
    assert current.renderer_config == current.config
    assert current.config_accepted == token
    assert current.config_applied == token
    assert current.config_pending == nil
    assert :sys.get_state(current.lifecycle).config.config.output.display == :on
    assert Emerge.renderer(pid) == renderer
    refute_receive {:configured, _}, 20
    send(pid, {:emerge_skia_display_wake, token, :double_tap})
    _ = runtime(pid)
    refute_receive {:rendered, _}, 20
    refute_receive {:unexpected_user_message, _}, 20
    refute_receive {:observed_input, _}, 20
  end

  test "touch observation does not subscribe to wake input" do
    pid = start_supervised!({Screen, observer: self(), events: [:touch]})
    _ = Emerge.renderer(pid)
    wake(pid, :double_tap, complete?: true)
    assert runtime(pid).config.output.display == :on
    assert_receive {:rendered, _}
    refute_receive {:observed_input, _}, 20
  end

  test "wake subscriptions can be enabled and disabled live without disabling wake" do
    pid = start_supervised!({Screen, observer: self()})
    renderer = Emerge.renderer(pid)
    wake(pid, :double_tap, complete?: true)
    assert runtime(pid).config.output.display == :on
    refute_receive {:observed_input, _}, 20

    for {events, subscribed?} <- [{[:display_wake], true}, {[], false}] do
      send(pid, {:change, %{forced: :off, events: events}})
      assert_receive {:configured, %Config{output: %Config.DRM{display: :off}}}
      assert runtime(pid).config.input.events == events
      send(pid, {:change, %{forced: nil}})
      _ = runtime(pid)
      token = wake(pid, :double_tap, complete?: true)
      current = runtime(pid)
      assert current.config.output.display == :on
      assert current.config_applied == token
      assert :sys.get_state(current.lifecycle).config.config.output.display == :on
      assert Emerge.renderer(pid) == renderer

      if subscribed? do
        assert_receive {:observed_input, {:display_wake, :double_tap}}
      else
        refute_receive {:observed_input, _}, 20
      end
    end
  end

  test "recovery replays the accepted on intent rather than the original off startup" do
    pid = start_supervised!({Screen, observer: self()})
    _ = Emerge.renderer(pid)
    wake(pid, :double_tap, complete?: true)
    assert_receive {:rendered, _}
    old = runtime(pid)
    assert :ok = Emerge.Runtime.Viewport.Lifecycle.discard(old.lifecycle)

    send(
      pid,
      {:emerge_viewport, :renderer_unavailable, old.lifecycle, old.renderer_generation, :ok}
    )

    assert runtime(pid).renderer == nil
    send(old.lifecycle, :retry)
    assert_receive {:rendered, _}
    current = runtime(pid)
    assert current.renderer != old.renderer
    assert current.config.output.display == :on
    assert current.config_accepted == nil
    assert Agent.get(current.renderer, & &1.config.output.display) == :on

    send(
      pid,
      {:emerge_viewport_renderer, old.renderer_generation,
       {:emerge_skia_display_wake, {101, 10}, :double_tap}}
    )

    assert runtime(pid).renderer == current.renderer
  end

  for gesture <- [:single_tap, :double_tap, :long_press] do
    test "#{gesture} updates state-driven config through subscribed handle_input" do
      gesture = unquote(gesture)
      pid = start_supervised!({StateScreen, observer: self(), wake_on_touch: gesture})
      _ = Emerge.renderer(pid)
      assert runtime(pid).config.input.events == [:display_wake, :key]
      token = wake(pid, gesture)
      assert_receive {:wake_input, ^gesture}
      assert_receive :state_rendered
      assert runtime(pid).config.output.display == :on
      assert runtime(pid).config_pending == token
      assert :sys.get_state(pid).sleeping? == false

      send(pid, {:emerge_skia_display_wake, token, gesture})
      _ = runtime(pid)
      refute_receive {:wake_input, _}, 20

      send(pid, {:emerge_skia_event, {:key, {:escape, 1, []}}})
      assert_receive {:other_input, {:key, {:escape, :press, []}}}
    end
  end

  test "all_events subscribes to wake input" do
    pid = start_supervised!({StateScreen, observer: self(), events: Config.Input.all_events()})
    _ = Emerge.renderer(pid)
    wake(pid, :long_press)
    assert_receive {:wake_input, :long_press}
    assert runtime(pid).config.output.display == :on
  end

  test "unsubscribed state-driven config retains its authority without invoking input" do
    pid = start_supervised!({StateScreen, observer: self(), events: []})
    _ = Emerge.renderer(pid)
    token = wake(pid, :long_press)
    assert_receive {:configured, %Config{output: %Config.DRM{display: :off}}}
    assert runtime(pid).config_accepted > token
    assert :sys.get_state(pid).sleeping?
    refute_receive {:wake_input, _}, 20
    refute_receive :state_rendered, 20
  end

  test "nil disables wake input notifications even when subscribed and does not add a callback" do
    pid = start_supervised!({StateScreen, observer: self(), wake_on_touch: nil})
    _ = Emerge.renderer(pid)
    send(pid, {:emerge_skia_display_wake, {101, 1}, :long_press})
    assert runtime(pid).config.output.display == :off
    refute_receive {:wake_input, _}, 20
    refute Keyword.has_key?(Emerge.behaviour_info(:callbacks), :handle_display_wake)
    refute function_exported?(StateScreen, :handle_display_wake, 2)
  end

  test "state-driven explicit off wins instead of a hidden display override" do
    pid = start_supervised!({StateScreen, observer: self(), veto?: true})
    _ = Emerge.renderer(pid)
    token = wake(pid, :long_press)
    assert_receive {:wake_input, :long_press}
    assert_receive {:configured, %Config{output: %Config.DRM{display: :off}}}
    refute_receive :state_rendered, 20
    assert runtime(pid).config.output.display == :off
    assert runtime(pid).config_accepted > token
  end

  test "wake input exceptions are contained and normal config authority is retained" do
    pid = start_supervised!({StateScreen, observer: self(), fail?: true})
    _ = Emerge.renderer(pid)
    assert capture_log(fn -> wake(pid, :long_press) end) =~ "wake input failed"
    assert Process.alive?(pid)
    assert runtime(pid).config.output.display == :off
    refute_receive :state_rendered, 20
  end

  test "disabled policy, wrong gesture and obsolete renderer generation cannot wake" do
    pid = start_supervised!({Screen, observer: self()})
    _ = Emerge.renderer(pid)

    send(
      pid,
      {:emerge_viewport_renderer, make_ref(), {:emerge_skia_display_wake, {101, 1}, :double_tap}}
    )

    send(pid, {:emerge_skia_display_wake, {101, 1}, :single_tap})
    assert runtime(pid).config.output.display == :off
    send(pid, {:change, %{policy: nil}})
    assert_receive {:configured, %Config{output: %Config.DRM{wake_on_touch: nil}}}
    assert runtime(pid).config.output.wake_on_touch == nil
    send(pid, {:emerge_skia_display_wake, {101, 2}, :double_tap})
    assert runtime(pid).config.output.display == :off
    refute_receive {:rendered, _}, 20
  end

  test "newer explicit off and completed revisions reject late wake notifications" do
    pid = start_supervised!({Screen, observer: self()})
    _ = Emerge.renderer(pid)
    old_token = wake(pid, :double_tap, complete?: true)
    assert_receive {:rendered, _}
    send(pid, {:change, %{forced: :off}})
    assert_receive {:configured, %Config{output: %Config.DRM{display: :off}}}
    current = runtime(pid)
    assert current.config.output.display == :off
    assert current.config_accepted > old_token
    send(pid, {:emerge_skia_config, current.config_pending, :ok})
    send(pid, {:emerge_skia_display_wake, old_token, :double_tap})
    send(pid, {:emerge_skia_display_wake, {999, 50}, :double_tap})
    assert runtime(pid).config.output.display == :off
    assert runtime(pid).config_pending == nil
    refute_receive {:rendered, _}, 20
  end
end
