defmodule Emerge.EventDocumentationTest do
  use ExUnit.Case, async: true

  alias Emerge.UI.Event

  @root Path.expand("../..", __DIR__)
  @pointer_kinds [:click, :mouse_down, :mouse_up, :mouse_move, :mouse_enter, :mouse_leave]

  test "public event docs cover payload-bearing helpers and raw/local coordinate distinctions" do
    {:docs_v1, _, _, _, %{"en" => event_docs}, _, _} = Code.fetch_docs(Event)
    native_docs = function_docs(EmergeSkia, :set_input_target, 2)

    for kind <- @pointer_kinds do
      helper = String.to_existing_atom("on_#{kind}")
      assert event_docs =~ "`#{helper}/1`"
      assert function_docs(Event, helper, 1) =~ "%{x: float(), y: float()}"
      assert native_docs =~ "`:#{kind}`"
    end

    assert event_docs =~ "## Captured Dragging"
    assert event_docs =~ "**not** pointer coordinates"
    assert native_docs =~ "captured source's current geometry"

    for module <- [Emerge, Event, Emerge.Input, Emerge.UI.Input, Emerge.UI.Nearby, EmergeSkia] do
      {:docs_v1, _, _, _, %{"en" => module_docs}, _, entries} = Code.fetch_docs(module)
      docs = [module_docs | Enum.flat_map(entries, &english_docs/1)]

      for source <- Enum.flat_map(docs, &code_blocks/1) do
        assert {:ok, _} = Code.string_to_quoted(source),
               "invalid Elixir example in #{inspect(module)}: #{source}"
      end
    end
  end

  test "public Event type includes the captured drag attribute" do
    {:ok, types} = Code.Typespec.fetch_types(Event)

    {:type, {:t, {:type, _, :union, members}, []}} =
      Enum.find(types, &match?({:type, {:t, _, []}}, &1))

    assert Enum.any?(members, &match?({:user_type, _, :drag_attr, []}, &1))
  end

  test "mouse tutorial handler preserves logical floats and requests a redraw" do
    source =
      "guides/tutorials/handle_input.md"
      |> read()
      |> code_blocks()
      |> Enum.find(&String.contains?(&1, "def handle_info({:moved,"))

    module = Emerge.DocumentationExamples.MousePosition
    compile_fragment(module, source)
    assert apply(module, :render, [%{}]).attrs.on_mouse_move == {self(), :moved}
    position = %{x: 35.5, y: -2.25}

    assert {:noreply, state} =
             apply(module, :handle_info, [{:moved, position}, runtime_state(module)])

    assert state.pointer == {35.5, -2.25}
    assert state.__emerge__.dirty?
  end

  test "drag reference and runnable tutorial handle every phase including cancellation" do
    reference_source = Event |> function_docs(:on_drag, 1) |> code_blocks() |> List.first()
    reference = Emerge.DocumentationExamples.DragReference
    compile_fragment(reference, reference_source)

    tutorial_source =
      "guides/tutorials/handle_input.md"
      |> read()
      |> code_blocks()
      |> Enum.find(&String.contains?(&1, "defmodule MyApp.PointerPad do"))
      |> String.replace("MyApp.PointerPad", "Emerge.DocumentationExamples.DragTutorial")

    Code.compile_string(tutorial_source)
    tutorial = Emerge.DocumentationExamples.DragTutorial

    assert {:ok, %{pointer: nil, dragging?: false}, %Emerge.Config{} = config} =
             apply(tutorial, :mount, [[]])

    assert :ok = EmergeSkia.validate_config!(config)

    for module <- [reference, tutorial] do
      state = runtime_state(module)
      tree = apply(module, :render, [state])
      {_binary, diff, assigned} = Emerge.Engine.encode_full(Emerge.Engine.diff_state_new(), tree)
      pad = if module == reference, do: assigned, else: hd(assigned.children)
      id = <<pad.id::64>>
      assert Emerge.Engine.lookup_event(diff, id, :drag) == {:ok, {self(), :picked}}

      ended =
        Enum.reduce([:start, :move, :end], state, fn phase, previous ->
          message = {:picked, %{phase: phase, x: 195.75, y: -2.25}}
          assert {:noreply, next} = apply(module, :handle_info, [message, previous])
          assert next.pointer == {195.75, -2.25}
          assert next.dragging? == (phase != :end)
          assert next.__emerge__.dirty?
          next
        end)

      assert {:noreply, started} =
               apply(module, :handle_info, [
                 {:picked, %{phase: :start, x: 35.5, y: 20.25}},
                 ended
               ])

      assert {:noreply, cancelled} =
               apply(module, :handle_info, [
                 {:picked, %{phase: :cancel, x: 210.0, y: 130.0}},
                 started
               ])

      refute cancelled.dragging?
      assert cancelled.pointer == {35.5, 20.25}
    end
  end

  defp compile_fragment(module, source) do
    Code.compile_string("""
    defmodule #{inspect(module)} do
      use Emerge.UI
      alias Emerge, as: Viewport
      #{source}
    end
    """)
  end

  defp runtime_state(module) do
    %{
      pointer: nil,
      dragging?: false,
      __emerge__: %Emerge.Runtime.Viewport.State{module: module, flush_scheduled?: true}
    }
  end

  defp function_docs(module, name, arity) do
    {:docs_v1, _, _, _, _, _, entries} = Code.fetch_docs(module)
    {_, _, _, %{"en" => docs}, _} = List.keyfind(entries, {:function, name, arity}, 0)
    docs
  end

  defp english_docs({_, _, _, %{"en" => docs}, _}), do: [docs]
  defp english_docs(_), do: []

  defp read(path), do: File.read!(Path.join(@root, path))

  defp code_blocks(content) do
    Regex.scan(~r/```elixir[^\n]*\n(.*?)\n```/s, content, capture: :all_but_first)
    |> Enum.map(fn [source] -> source end)
  end
end
