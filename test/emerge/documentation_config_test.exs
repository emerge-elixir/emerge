defmodule Emerge.DocumentationConfigTest do
  use ExUnit.Case, async: true
  alias Emerge.Config

  @root Path.expand("../..", __DIR__)
  @config_guide "guides/reference/viewport-configuration.md"
  @records [
    Config.Window,
    Config.DRM,
    Config.Backlight,
    Config.Headless,
    Config.BinaryFrames,
    Config.PrimeFrames,
    Config.Renderer,
    Config.RendererCache,
    Config.PaintLayerCache,
    Config.Input,
    Config.Cursor,
    Config.CursorIcon,
    Config.Assets,
    Config.Font,
    Config.RuntimePaths,
    Config.AssetCache,
    Config.Diagnostics,
    Config.Runtime
  ]

  test "each public config record explains every field and has a valid placement example" do
    for module <- @records do
      {:docs_v1, _, _, _, %{"en" => doc}, _, _} = Code.fetch_docs(module)

      assert String.contains?(doc, "## Fields"),
             "missing field documentation in #{inspect(module)}"

      for {field, _type} <- module.__schema__() do
        assert String.contains?(doc, "- `#{field}` —"),
               "missing explanation for #{inspect(module)}.#{field}"
      end

      examples =
        Enum.filter(code_blocks(doc), &String.starts_with?(String.trim(&1), "%Emerge.Config{"))

      assert examples != [], "missing placement example in #{inspect(module)}"

      for source <- examples do
        {literal, _binding} = Code.eval_string(source, [], __ENV__)
        config = Config.Normalize.config!(literal, Emerge)
        assert :ok = EmergeSkia.validate_config!(config)
      end
    end
  end

  test "migration config examples validate without opening a renderer" do
    sources = "guides/migrations/0.5.md" |> read() |> code_blocks()
    examples = Enum.filter(sources, &String.starts_with?(String.trim(&1), "%Emerge.Config{"))
    assert length(examples) == 6

    for source <- sources, do: assert(match?({:ok, _}, Code.string_to_quoted(source)))

    for source <- examples do
      {literal, _binding} = Code.eval_string(source, [], __ENV__)
      config = Config.Normalize.config!(literal, Emerge)
      assert :ok = EmergeSkia.validate_config!(config)
    end
  end

  test "configuration reference examples normalize and adapt without opening hardware" do
    examples =
      Regex.scan(
        ~r/<!-- emerge-config:(\w+) -->\s*```elixir\n(.*?)\n```/s,
        read(@config_guide)
      )

    assert Enum.map(examples, fn [_, name, _] -> name end) == ["window", "drm", "binary", "prime"]

    for [_, name, source] <- examples do
      {literal, _binding} = Code.eval_string(source, [], __ENV__)
      config = Config.Normalize.config!(literal, Emerge)
      assert %Config{} = config
      assert :ok = EmergeSkia.validate_config!(config)
      assert config == Config.Normalize.config!(config)
      native = config |> Config.Native.options() |> EmergeSkia.Options.build_start_native_opts!()
      assert native.input_log == false

      case name do
        "window" ->
          assert config.output.size == {1024, 768}
          assert config.input.events == [:key, :resize]

        "drm" ->
          assert native.drm_power == %{display_on: true, brightness: 0.5}
          assert native.drm_backlight == "/sys/class/backlight/panel"
          assert native.drm_hotplug_interval_ms == nil

        "binary" ->
          assert native.backend == "headless"
          assert native.headless.pixel_format == "bw1"
          assert native.headless.dither
          assert native.target_fps == 30

        "prime" ->
          assert native.headless.mode == "prime"
          assert native.headless.prime.drm_node == "/dev/dri/renderD128"
          assert native.headless.prime.max_in_flight == 2
      end
    end
  end

  test "current guide code parses and never recommends keyword or positional renderer startup" do
    paths = ["README.md" | Path.wildcard(Path.join(@root, "guides/**/*.md"))]

    for path <- paths,
        not String.ends_with?(path, "/migrations/0.4.md"),
        not String.ends_with?(path, "/migrations/0.5.md"),
        source <- code_blocks(read(path)) do
      parsed = Code.string_to_quoted(source)
      assert match?({:ok, _}, parsed), "invalid Elixir in #{path}: #{inspect(parsed)}\n#{source}"
      {:ok, quoted} = parsed

      Macro.prewalk(quoted, fn
        {{:., _, [{:__aliases__, _, [:EmergeSkia]}, :start]}, _, args} = call ->
          assert length(args) == 1, "positional renderer startup in #{path}"
          refute is_list(hd(args)), "keyword renderer startup in #{path}"
          call

        other ->
          other
      end)
    end
  end

  test "counter tutorial returns typed config and interprets only its mount arguments" do
    source =
      "guides/tutorials/set_up_viewport.md"
      |> read()
      |> code_blocks()
      |> Enum.find(&String.contains?(&1, "defmodule MyApp.View.Counter do"))
      |> String.replace("MyApp.View.Counter", "Emerge.DocumentationExamples.Counter")

    Code.compile_string(source)
    module = Emerge.DocumentationExamples.Counter
    assert {:ok, %{count: 0}, %Config{output: output}} = apply(module, :mount, [[]])
    assert output.title == "Counter"
    assert output.size == {800, 600}

    assert {:ok, %{count: 0}, %Config{output: changed}} =
             apply(module, :mount, [[title: "Custom", size: {1024, 768}, width: 1]])

    assert changed.title == "Custom"
    assert changed.size == {1024, 768}
  end

  test "input tutorial uses atom subscriptions and typed key actions" do
    source =
      "guides/tutorials/handle_input.md"
      |> read()
      |> code_blocks()
      |> Enum.find(&String.contains?(&1, "defmodule MyApp.KeyboardWindow do"))
      |> String.replace("MyApp.KeyboardWindow", "Emerge.DocumentationExamples.Keyboard")

    Code.compile_string(source)
    module = Emerge.DocumentationExamples.Keyboard
    assert {:ok, literal} = apply(module, :mount, [[]])
    config = Config.Normalize.config!(literal, Emerge)
    assert config.input.events == [:key, :pointer_button, :resize]

    assert {:stop, :normal, %{}} =
             apply(module, :handle_input, [{:key, {:escape, :press, []}}, %{}])

    assert {:noreply, %{}} =
             apply(module, :handle_input, [{:key, {:escape, :release, []}}, %{}])
  end

  defp read(path), do: File.read!(Path.expand(path, @root))

  defp code_blocks(content) do
    Regex.scan(~r/```elixir[^\n]*\n(.*?)\n```/s, content, capture: :all_but_first)
    |> Enum.map(fn [source] -> source end)
  end
end
