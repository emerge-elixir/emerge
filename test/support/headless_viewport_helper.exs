defmodule Emerge.TestSupport.HeadlessViewport do
  @moduledoc false
  use Emerge

  # A normal application. Build the declaration in render/1 so the default
  # listener address is the viewport, just as it would be in an application.
  @impl Viewport
  def mount(opts) do
    target = Keyword.fetch!(opts, :target)

    {:ok, %{target: target, scene: Keyword.fetch!(opts, :scene)},
     %Emerge.Config{
       output: %Emerge.Config.Headless{target: target, size: Keyword.fetch!(opts, :size)},
       renderer: [api: :raster] ++ Keyword.take(opts, [:target_fps]),
       input: [events: Keyword.get(opts, :events, [])],
       assets: [otp_app: :emerge]
     }}
  end

  @impl Viewport
  def render(state), do: state.scene.()

  @impl Viewport
  def handle_input(event, state) do
    send(state.target, {:raw, event})
    {:noreply, state}
  end

  @impl Viewport
  def handle_info({:scene, build}, state),
    do: {:noreply, Viewport.rerender(%{state | scene: build})}

  def handle_info(message, state) do
    send(state.target, {:application, message})
    {:noreply, state}
  end
end

defmodule Emerge.TestSupport.Headless do
  @moduledoc false
  import ExUnit.Assertions
  import ExUnit.Callbacks, only: [start_supervised!: 1]
  alias Emerge.TestSupport.HeadlessViewport

  def mount(size, scene, events \\ [], opts \\ []) do
    pid =
      start_supervised!(
        Supervisor.child_spec(
          {HeadlessViewport, [size: size, target: self(), scene: scene, events: events] ++ opts},
          id: make_ref(),
          restart: :temporary
        )
      )

    {pid, Emerge.renderer(pid)}
  end

  def pixels(timeout \\ 1500) do
    assert_receive {:emerge_skia_frame,
                    %VideoInterop.Frame{
                      coded_width: w,
                      coded_height: h,
                      storage: %VideoInterop.Binary{data: pixels}
                    } = frame},
                   timeout

    assert :ok = VideoInterop.validate(frame)
    assert byte_size(pixels) == w * h * 4
    pixels
  end

  # Some inputs produce more than one real frame (for example, focus followed
  # by an application rerender). Wait for the visual result of this step.
  def pixels_matching(predicate, timeout \\ 1500) do
    until_pixels(predicate, System.monotonic_time(:millisecond) + timeout)
  end

  defp until_pixels(predicate, deadline) do
    remaining = deadline - System.monotonic_time(:millisecond)
    assert remaining > 0, "headless output did not reach the expected pixels"
    pixels = pixels(remaining)
    if predicate.(pixels), do: pixels, else: until_pixels(predicate, deadline)
  end

  def move(renderer, x, y), do: EmergeSkia.send_input(renderer, {:pointer_move, {x, y}})

  def button(renderer, action, x, y),
    do: EmergeSkia.send_input(renderer, {:pointer_button, {:left, action, [], {x, y}}})
end

defmodule Emerge.TestSupport.BoxAssertions do
  @moduledoc false
  import ExUnit.Assertions
  alias Emerge.TestSupport.Headless

  # Literal outer rectangles, back to front. No use of the layout decoder.
  def frame({w, h}, boxes) do
    for y <- 0..(h - 1), x <- 0..(w - 1), into: <<>> do
      color =
        Enum.reduce(boxes, 0, fn {left, top, width, height, color}, behind ->
          if x >= left and x < left + width and y >= top and y < top + height,
            do: color,
            else: behind
        end)

      <<color::32>>
    end
  end

  def button(r, phase, {x, y}) do
    assert :ok = Headless.button(r, phase, x, y)
    assert_receive {:raw, {:pointer_button, {:left, ^phase, [], {^x, ^y}}}}
  end

  def click(r, point, target, {lx, ly}) do
    for phase <- [:press, :release], do: button(r, phase, point)
    assert_receive {:application, {^target, %{x: ^lx, y: ^ly}}}
  end

  def fresh(size, scene, expected) do
    Headless.mount(size, scene)
    assert Headless.pixels() == expected
  end
end
