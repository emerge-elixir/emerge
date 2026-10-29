defmodule Emerge.DocumentationConfigTest do
  use ExUnit.Case, async: true
  alias Emerge.Config

  @root Path.expand("../..", __DIR__)
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

  test "release documentation contains tutorials, not internal reference guides or links" do
    extras =
      Emerge.Mix.Docs.config(
        @root,
        Mix.Project.config()[:version],
        Mix.Project.config()[:source_url],
        %{"EMERGE_INCLUDE_INTERNAL_DOCS" => "false"}
      )[:extras]

    refute File.exists?(Path.join(@root, "guides/reference/viewport-configuration.md"))
    assert "guides/tutorials/set_up_viewport.md" in extras
    assert "guides/migrations/0.6.md" in extras
    refute "guides/tutorials/configure_native_renderer.md" in extras
    refute "guides/tutorials/configure_nerves_graphics.md" in extras
    {:docs_v1, _, _, _, %{"en" => viewport_docs}, _, _} = Code.fetch_docs(Emerge)
    assert viewport_docs =~ "## Viewport configuration"
    assert viewport_docs =~ "### Native build support versus runtime selection"
    assert viewport_docs =~ "### Nerves and device defaults"

    assert read("guides/tutorials/set_up_viewport.md") =~
             "Emerge.html#module-viewport-configuration"

    refute Enum.any?(extras, &String.starts_with?(&1, ["guides/reference/", "guides/internals/"]))

    for path <- extras do
      refute Regex.match?(~r/\]\([^)]*(?:reference|internals)\//, read(path)),
             "public documentation must not link to internal guides: #{path}"
    end

    internal_pages =
      [
        "viewport-configuration.html"
        | Path.wildcard(Path.join(@root, "guides/reference/*.md"))
          |> Enum.map(&(Path.basename(&1, ".md") <> ".html"))
      ]

    for module <- [Emerge, Config, Emerge.Runtime.Viewport, EmergeSkia] do
      {:docs_v1, _, _, _, %{"en" => docs}, _, _} = Code.fetch_docs(module)
      for page <- internal_pages, do: refute(String.contains?(docs, page))
    end
  end

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

  test "0.6 migration handlers accept float positions and appended tuple fields" do
    sources = "guides/migrations/0.6.md" |> read() |> code_blocks()
    for source <- sources, do: assert(match?({:ok, _}, Code.string_to_quoted(source)))

    source = Enum.find(sources, &String.contains?(&1, "def handle_info({:clicked, _position}"))
    module = Emerge.DocumentationExamples.PointerMigrationHandlers
    Code.compile_string("defmodule #{inspect(module)} do\n#{source}\nend")
    position = %{x: 35.5, y: -2.25}

    assert {:noreply, %{clicks: 1}} =
             apply(module, :handle_info, [{:clicked, position}, %{clicks: 0}])

    assert {:noreply, %{pointer: {35.5, -2.25}}} =
             apply(module, :handle_info, [{:pointer, :pad, position}, %{}])
  end

  test "0.6 migration adapter retains fixed arguments and delegates other payloads" do
    source =
      "guides/migrations/0.6.md"
      |> read()
      |> code_blocks()
      |> Enum.find(&String.contains?(&1, "def wrap_payload({:solve_event, event, argument}"))

    module = Emerge.DocumentationExamples.PointerMigrationAdapter
    Code.compile_string("defmodule #{inspect(module)} do\n#{source}\nend")
    position = %{x: 35.5, y: 20.25}

    for kind <- [:click, :mouse_down, :mouse_up, :mouse_move, :mouse_enter, :mouse_leave, :drag] do
      payload = if kind == :drag, do: Map.put(position, :phase, :move), else: position

      assert apply(module, :wrap_payload, [{:solve_event, :moved, :pad}, payload, kind]) ==
               {:solve_event, :moved, {:pad, payload}}
    end

    assert apply(module, :wrap_payload, [{:solve_event, :moved}, position, :mouse_move]) ==
             {:solve_event, :moved, position}

    assert apply(module, :wrap_payload, [:changed, "text", :change]) == {:changed, "text"}
  end

  test "configuration moduledoc examples normalize and adapt without opening hardware" do
    {:docs_v1, _, _, _, %{"en" => config_docs}, _, _} = Code.fetch_docs(Config)

    examples =
      Regex.scan(
        ~r/<!-- emerge-config:(\w+) -->\s*```elixir\n(.*?)\n```/s,
        config_docs
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
          refute String.contains?(path, "guides/tutorials/"),
                 "tutorial should configure its viewport rather than start another renderer: #{path}"

          assert length(args) == 1, "positional renderer startup in #{path}"
          refute is_list(hd(args)), "keyword renderer startup in #{path}"
          call

        other ->
          other
      end)
    end
  end

  test "nearby docs link to an existing tutorial section" do
    {:docs_v1, _, _, _, %{"en" => docs}, _, _} = Code.fetch_docs(Emerge.UI.Nearby)
    [anchor] = Regex.run(~r/describe_ui\.md#([\w-]+)/, docs, capture: :all_but_first)

    headings =
      ~r/^## (.+)$/m
      |> Regex.scan(read("guides/tutorials/describe_ui.md"), capture: :all_but_first)
      |> Enum.map(fn [heading] -> heading |> String.downcase() |> String.replace(" ", "-") end)

    assert anchor in headings
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

  test "asset tutorial config examples use typed records and valid settings" do
    examples =
      Regex.scan(
        ~r/<!-- emerge-assets:(\w+) -->\s*```elixir\n(.*?)\n```/s,
        read("guides/tutorials/use_assets.md")
      )

    assert Enum.map(examples, fn [_, name, _] -> name end) ==
             ["fonts", "runtime", "cache", "diagnostics"]

    for [_, name, source] <- examples do
      {literal, _binding} = Code.eval_string(source, [], __ENV__)
      assert %Config.Assets{} = literal.assets
      assert %Config.Diagnostics{} = literal.diagnostics
      config = Config.Normalize.config!(literal)
      assert config == literal
      assert :ok = EmergeSkia.validate_config!(config)

      case name do
        "fonts" ->
          assert Enum.all?(literal.assets.fonts, &match?(%Config.Font{}, &1))

          assert Enum.map(config.assets.fonts, &{&1.weight, &1.italic}) ==
                   [{400, false}, {700, false}, {400, true}]

        "runtime" ->
          assert %Config.RuntimePaths{enabled: true, allowlist: ["/data/photos"]} =
                   literal.assets.runtime_paths

        "cache" ->
          assert literal.assets.decode_at_size
          assert %Config.AssetCache{max_entries: 32, max_bytes: 33_554_432} = literal.assets.cache

        "diagnostics" ->
          assert literal.diagnostics.logs == [:stats]
      end
    end
  end

  test "asset tutorial viewport and verified-path view build the documented UI" do
    sources = "guides/tutorials/use_assets.md" |> read() |> code_blocks()

    viewport_source =
      sources
      |> Enum.find(&String.contains?(&1, "defmodule MyApp.View.Gallery do"))
      |> String.replace("MyApp.View.Gallery", "Emerge.DocumentationExamples.Gallery")
      |> use_test_assets()

    view_source =
      sources
      |> Enum.find(&String.contains?(&1, "defmodule MyApp.UI.Gallery do"))
      |> String.replace("MyApp.UI.Gallery", "Emerge.DocumentationExamples.GalleryView")
      |> use_test_assets()

    Code.compile_string(viewport_source)
    Code.compile_string(view_source)
    viewport = Emerge.DocumentationExamples.Gallery
    view = Emerge.DocumentationExamples.GalleryView

    assert {:ok, %Config{assets: %Config.Assets{otp_app: :emerge}} = config} =
             apply(viewport, :mount, [[]])

    assert :ok = EmergeSkia.validate_config!(config)

    assert %Emerge.Engine.Element{type: :column, children: [image, svg]} =
             apply(viewport, :render, [])

    assert image.attrs.image_src == "sample_assets/static.jpg"
    assert svg.attrs.image_src == "sample_assets/template_cloud.svg"

    assert %Emerge.Engine.Element{type: :column, children: [verified_image, verified_svg]} =
             apply(view, :layout, [])

    assert %Emerge.Assets.Ref{path: "sample_assets/static.jpg", verified?: true} =
             verified_image.attrs.image_src

    assert %Emerge.Assets.Ref{path: "sample_assets/template_cloud.svg", verified?: true} =
             verified_svg.attrs.image_src
  end

  test "counter title callback preserves window settings and follows state" do
    sources = "guides/tutorials/set_up_viewport.md" |> read() |> code_blocks()
    callback = Enum.find(sources, &String.contains?(&1, "def config(previous, state) do"))

    source =
      sources
      |> Enum.find(&String.contains?(&1, "defmodule MyApp.View.Counter do"))
      |> String.replace("MyApp.View.Counter", "Emerge.DocumentationExamples.CounterTitle")
      |> String.trim()
      |> String.replace_suffix("end", callback <> "\nend")

    module = Emerge.DocumentationExamples.CounterTitle
    Code.compile_string(source)
    previous = %Config{output: %Config.Window{size: {1024, 768}}}
    next = apply(module, :config, [previous, %{count: 3}])
    assert next.output.title == "Count: 3"
    assert next.output.size == {1024, 768}
    assert :ok = EmergeSkia.validate_config_change(previous, next)
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

  defp use_test_assets(source) do
    source
    |> String.replace(":my_app", ":emerge")
    |> String.replace("images/logo.png", "sample_assets/static.jpg")
    |> String.replace("icons/check.svg", "sample_assets/template_cloud.svg")
  end

  defp read(path), do: File.read!(Path.expand(path, @root))

  defp code_blocks(content) do
    Regex.scan(~r/```elixir[^\n]*\n(.*?)\n```/s, content, capture: :all_but_first)
    |> Enum.map(fn [source] -> source end)
  end
end
