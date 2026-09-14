defmodule Emerge.MixProjectTest do
  use ExUnit.Case, async: true

  @root Path.expand("..", __DIR__)
  @build_env ~w(CC CXX CFLAGS CXXFLAGS CPPFLAGS LDFLAGS RUSTFLAGS SKIA_GN_ARGS
    BINDGEN_EXTRA_CLANG_ARGS CLANGCC CLANGCXX HOST_CC HOST_CXX SDKTARGETSYSROOT
    NERVES_SDK_SYSROOT NERVES_TOOLCHAIN EMERGE_SKIA_HOST_PYTHON
    CARGO_PROFILE_RELEASE_STRIP EMERGE_SOURCE_REVISION MIX_TARGET TARGET_ARCH
    TARGET_OS TARGET_ABI TARGET_VENDOR EMERGE_INCLUDE_INTERNAL_DOCS)

  test "project loads cold without runtime modules or optional dependencies" do
    root = fixture()
    config = probe(root)
    assert config.opts == []
    assert config.app == :emerge
    assert config.elixir == "~> 1.19"
    assert config.application == [extra_applications: [:logger]]
    assert {:rustler, "~> 0.38.0", optional: true} in config.deps
    assert config.cli[:preferred_envs][:"quality.fast"] == :test
    assert config.cli[:preferred_envs][:dialyzer] == :dev

    for {name, _} <- config.aliases,
        name == :bench or String.starts_with?(Atom.to_string(name), "bench.") do
      assert config.cli[:preferred_envs][name] == :dev
    end
  end

  test "compiler target precedence and cross-environment flags are preserved" do
    root = fixture()

    for {compiler, target} <- [
          {"armv6-nerves-linux-gnueabihf", "arm-unknown-linux-gnueabihf"},
          {"armv7-nerves-linux-gnueabihf", "armv7-unknown-linux-gnueabihf"},
          {"aarch64-nerves-linux-gnu", "aarch64-unknown-linux-gnu"},
          {"x86_64-nerves-linux-musl", "x86_64-unknown-linux-musl"},
          {"riscv64-nerves-linux-gnu", "riscv64gc-unknown-linux-gnu"}
        ] do
      cc = "/toolchain/bin/#{compiler}-gcc -O2"

      config =
        probe(root, %{
          "CC" => cc,
          "TARGET_ARCH" => "conflict",
          "TARGET_OS" => "linux",
          "RUSTFLAGS" => "--cfg caller",
          "CFLAGS" => "-mabi=lp64 -O2",
          "HOST_CC" => "/custom/cc",
          "CARGO_PROFILE_RELEASE_STRIP" => "none"
        })

      assert config.opts[:target] == target
      env = Map.new(config.opts[:env])

      assert env["CARGO_TARGET_#{target |> String.upcase() |> String.replace("-", "_")}_LINKER"] ==
               cc

      assert env["HOST_CC"] == "/custom/cc"
      assert env["CFLAGS"] == "-mabi=lp64 -O2"
      assert env["CARGO_PROFILE_RELEASE_STRIP"] == "none"
      assert String.starts_with?(env["RUSTFLAGS"], "--cfg caller -Lnative=")

      assert String.contains?(env["RUSTFLAGS"], "-crt-static") ==
               String.ends_with?(target, "musl")
    end
  end

  test "generic target variables retain their existing preparation policy" do
    root = fixture()

    config =
      probe(root, %{"TARGET_ARCH" => "riscv64", "TARGET_OS" => "linux", "TARGET_ABI" => ""})

    assert config.opts[:target] == "riscv64gc-unknown-linux-gnu"

    assert Map.new(config.opts[:env])["SKIA_GN_ARGS"] ==
             "skia_use_fontconfig=false skia_use_system_freetype2=false"
  end

  test "full SDK preparation supports direct and opt/ext-toolchain layouts" do
    root = fixture()
    prefix = "aarch64-nerves-linux-gnu"
    python = Path.join(root, "host python interpreter")

    File.write!(
      python,
      "#!/bin/sh\nprintf '%s|%s|%s|%s' \"${PYTHONHOME-unset}\" \"${PYTHONPATH-unset}\" \"${LD_LIBRARY_PATH-unset}\" \"$1\"\n"
    )

    File.chmod!(python, 0o755)
    bin = Path.join(root, "bin")
    File.mkdir_p!(bin)

    for name <- ["clang", "clang++"] do
      File.write!(Path.join(bin, name), "#!/bin/sh\nexit 0\n")
      File.chmod!(Path.join(bin, name), 0o755)
    end

    for layout <- ["direct", "opt/ext-toolchain"] do
      toolchain = Path.join(root, layout)
      includes = Path.join([toolchain, prefix, "include/c++/15.3.1"])
      File.mkdir_p!(Path.join(includes, prefix))
      File.mkdir_p!(Path.join(toolchain, "lib/gcc"))
      declared_toolchain = if layout == "direct", do: toolchain, else: root

      config =
        probe(root, %{
          "CC" => "/toolchain/bin/#{prefix}-gcc",
          "NERVES_TOOLCHAIN" => declared_toolchain,
          "NERVES_SDK_SYSROOT" => "/sdk/sysroot",
          "EMERGE_SKIA_HOST_PYTHON" => python,
          "PATH" => bin <> ":" <> System.fetch_env!("PATH"),
          "CFLAGS" => "-O2 -mabi=lp64",
          "CXXFLAGS" => "-O3 -mabi=lp64",
          "BINDGEN_EXTRA_CLANG_ARGS" => "-DCALLER"
        })

      env = Map.new(config.opts[:env])
      assert env["CFLAGS"] == "-O2"
      assert env["CXXFLAGS"] == "-O3 -I#{includes} -I#{includes}/#{prefix} -Wno-invalid-constexpr"
      assert env["BINDGEN_EXTRA_CLANG_ARGS"] == "-DCALLER " <> env["CXXFLAGS"]

      assert env["CC"] ==
               "#{bin}/clang --sysroot=/sdk/sysroot --gcc-toolchain=#{toolchain} -I#{includes} -I#{includes}/#{prefix}"

      assert env["CXX"] ==
               "#{bin}/clang++ --sysroot=/sdk/sysroot --gcc-toolchain=#{toolchain} -I#{includes} -I#{includes}/#{prefix} -Wno-invalid-constexpr"

      assert env["SDKTARGETSYSROOT"] == "/sdk/sysroot"
      tools = Path.join(root, "native/emerge_skia/target/nerves-host-tools")
      assert String.starts_with?(env["PATH"], tools <> ":")

      for name <- ["python", "python3"] do
        wrapper = Path.join(tools, name)
        assert Bitwise.band(File.stat!(wrapper).mode, 0o777) == 0o755

        assert {"unset|unset|unset|argument with spaces", 0} =
                 System.cmd(wrapper, ["argument with spaces"],
                   env: [{"PYTHONHOME", "bad"}, {"PYTHONPATH", "bad"}, {"LD_LIBRARY_PATH", "bad"}]
                 )
      end
    end
  end

  test "full SDK still validates Python while loading project configuration" do
    root = fixture()

    {output, status} =
      run_probe(root, %{
        "CC" => "x86_64-nerves-linux-musl-gcc",
        "NERVES_SDK_SYSROOT" => "/sdk",
        "NERVES_TOOLCHAIN" => "/toolchain",
        "EMERGE_SKIA_HOST_PYTHON" => "relative/python"
      })

    assert status != 0
    assert output =~ "Nerves rust-skia builds require host Python at an absolute path"
  end

  test "docs preserve ordering, truthiness and the HTML-only callback" do
    root = fixture()

    for file <- ["README.md", "guides/migrations/0.4.md", "guides/internals/architecture.md"] do
      File.mkdir_p!(Path.dirname(Path.join(root, file)))
      File.write!(Path.join(root, file), "# Example\n")
    end

    for value <- [nil, "false", "0", "true", "", "False"] do
      config = probe(root, %{"EMERGE_INCLUDE_INTERNAL_DOCS" => value})
      internal? = value not in [nil, "false", "0"]

      assert config.extras ==
               ["README.md", "guides/migrations/0.4.md"] ++
                 if(internal?, do: ["guides/internals/architecture.md"], else: [])

      assert :Internals in config.groups == internal?
      assert config.html =~ "mermaid@11/dist/mermaid.esm.min.mjs"
      assert config.html =~ "DOMContentLoaded"
      assert config.non_html == ""
    end
  end

  test "package allowlist includes bootstrap helpers but excludes native build output" do
    root = fixture()

    for file <- [
          "native/emerge_skia/src/lib.rs",
          "native/emerge_skia/benches/layout.rs",
          "native/emerge_skia/support/tool.sh",
          "native/emerge_skia/target/cache.o",
          "native/emerge_skia/src/.hidden",
          "assets/ui-test.png",
          "checksum-test.exs"
        ] do
      path = Path.join(root, file)
      File.mkdir_p!(Path.dirname(path))
      File.write!(path, "fixture")
    end

    files = Emerge.Mix.Package.config(root, "https://example.test")[:files]
    assert "mix" in files
    assert Enum.count(files, &(&1 == "native/emerge_skia/Cross.toml")) == 1
    assert "native/emerge_skia/support/tool.sh" in files
    assert "native/emerge_skia/benches/layout.rs" in files
    assert "native/emerge_skia/src/lib.rs" in files
    assert "checksum-test.exs" in files
    assert "assets/ui-test.png" in files
    refute Enum.any?(files, &String.contains?(&1, "/target/"))
    refute "native/emerge_skia/src/.hidden" in files
  end

  test "new Mix runs see helper and target-map edits without cached helper modules" do
    root = fixture()
    env = %{"CC" => "example-nerves-linux-gnu-gcc"}
    assert probe(root, env).opts == []

    File.write!(
      Path.join(root, "mix/targets.exs"),
      inspect(%{"example-nerves-linux-gnu" => "riscv64gc-unknown-linux-gnu"})
    )

    assert probe(root, env).opts[:target] == "riscv64gc-unknown-linux-gnu"
    native = Path.join(root, "mix/native.exs")

    File.write!(
      native,
      String.replace(File.read!(native), "skia_use_fontconfig=false", "skia_use_fontconfig=true")
    )

    assert Map.new(probe(root, env).opts[:env])["SKIA_GN_ARGS"] =~ "skia_use_fontconfig=true"
  end

  test "compiled consumers track build inputs without runtime helper dependencies" do
    native_resources =
      Keyword.get_values(EmergeSkia.Native.__info__(:attributes), :external_resource)
      |> List.flatten()

    config_resources =
      Keyword.get_values(EmergeSkia.BuildConfig.__info__(:attributes), :external_resource)
      |> List.flatten()

    assert Path.join(@root, "mix/native.exs") in native_resources
    assert Path.join(@root, "mix/targets.exs") in native_resources
    assert Path.join(@root, "mix/targets.exs") in config_resources

    code = """
    false = Code.ensure_loaded?(Emerge.Mix.Native)
    false = Code.ensure_loaded?(Emerge.Mix.Docs)
    [:drm] = EmergeSkia.BuildConfig.default_compiled_backends(%{"CC" => "riscv64-nerves-linux-gnu-gcc"})
    [:wayland] = EmergeSkia.BuildConfig.default_compiled_backends(%{"TARGET_ARCH" => "x86_64", "TARGET_OS" => "linux"})
    false = Code.ensure_loaded?(Emerge.Mix.Native)
    IO.puts("runtime ready")
    """

    assert {"runtime ready\n", 0} =
             System.cmd(
               System.find_executable("elixir"),
               ["-pa", Application.app_dir(:emerge, "ebin"), "-e", code],
               stderr_to_stdout: true,
               env: [{"ERL_FLAGS", "+S 2:2"}]
             )
  end

  defp fixture do
    root =
      Path.join(System.tmp_dir!(), "emerge-mix-project-#{System.unique_integer([:positive])}")

    File.mkdir_p!(root)
    File.cp!(Path.join(@root, "mix.exs"), Path.join(root, "mix.exs"))

    File.cp_r!(Path.join(@root, "mix"), Path.join(root, "mix"))

    on_exit(fn -> File.rm_rf!(root) end)
    root
  end

  defp probe(root, env \\ %{}) do
    {output, status} = run_probe(root, env)
    assert status == 0, output
    refute output =~ "warning:", output
    root |> Path.join("result.etf") |> File.read!() |> :erlang.binary_to_term()
  end

  defp run_probe(root, env) do
    code = """
    for {key, value} <- #{inspect(env)}, is_binary(value), do: System.put_env(key, value)
    Mix.start()
    Code.compile_file("mix.exs")
    false = Code.ensure_loaded?(Rustler)
    false = Code.ensure_loaded?(ExDoc)
    false = Code.ensure_loaded?(EmergeSkia.Native)
    config = Emerge.MixProject.project()
    docs = config[:docs]
    result = %{opts: config[:rustler_opts], app: config[:app], elixir: config[:elixir],
      deps: config[:deps], aliases: config[:aliases], cli: Emerge.MixProject.cli(),
      application: Emerge.MixProject.application(), extras: docs[:extras],
      groups: Keyword.keys(docs[:groups_for_extras]),
      html: docs[:before_closing_body_tag].(:html), non_html: docs[:before_closing_body_tag].(:epub)}
    File.write!("result.etf", :erlang.term_to_binary(result))
    """

    System.cmd(System.find_executable("elixir"), ["-e", code],
      cd: root,
      stderr_to_stdout: true,
      env:
        Map.to_list(Map.merge(Map.new(@build_env, &{&1, nil}), env)) ++ [{"ERL_FLAGS", "+S 2:2"}]
    )
  end
end
