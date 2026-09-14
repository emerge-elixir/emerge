defmodule Emerge.MixPackageTest do
  use ExUnit.Case, async: true

  @moduletag :full_sweep
  @moduletag timeout: 120_000
  @root Path.expand("..", __DIR__)

  test "unpacked consumer bootstraps offline and recompiles changed build inputs" do
    root = Path.join(System.tmp_dir!(), "emerge-package-#{System.unique_integer([:positive])}")
    package = Path.join(root, "package")
    consumer = Path.join(root, "consumer")
    File.mkdir_p!(Path.join(consumer, "lib"))
    on_exit(fn -> File.rm_rf!(root) end)

    env =
      Enum.map(
        ~w(CC CXX TARGET_ARCH TARGET_OS TARGET_ABI TARGET_VENDOR MIX_TARGET
      NERVES_SDK_SYSROOT NERVES_TOOLCHAIN EMERGE_SKIA_HOST_PYTHON EMERGE_SKIA_BUILD
      EMERGE_INCLUDE_INTERNAL_DOCS RUSTLER_PRECOMPILED_FORCE_BUILD_ALL),
        &{&1, nil}
      ) ++
        [
          {"ERL_FLAGS", "+S 2:2"},
          {"MIX_ENV", "prod"},
          {"EMERGE_SKIA_CHECKSUM_ONLY", "1"},
          {"RUSTLER_PRECOMPILED_GLOBAL_CACHE_PATH", Path.join(root, "cache")}
        ]

    mix!(~w(hex.build --unpack --output) ++ [package], @root, env)

    for name <- ~w(native docs package targets) do
      assert File.regular?(Path.join(package, "mix/#{name}.exs"))
    end

    refute File.exists?(Path.join(package, "native/emerge_skia/target"))

    # Use already fetched dependency sources, but a fresh build/code path. No
    # registry access, Rustler, ExDoc or compiled checkout modules are required.
    deps =
      [{:emerge, [path: package]}] ++
        Enum.map([:rustler_precompiled, :video_interop, :jason, :castore], fn app ->
          {app, [path: Path.join([@root, "deps", Atom.to_string(app)]), override: true]}
        end)

    File.write!(Path.join(consumer, "mix.exs"), """
    defmodule Consumer.MixProject do
      use Mix.Project
      def project, do: [app: :consumer, version: "0.1.0", deps: #{inspect(deps)}]
    end
    """)

    File.write!(Path.join(consumer, "lib/consumer.ex"), """
    defmodule Consumer do
      use Emerge.UI
      def view, do: paragraph([width(fill())], [text("Package smoke")])
    end
    """)

    mix!(["deps.get"], consumer, env)
    mix!(["compile", "--warnings-as-errors"], consumer, env)
    mix!(["run", "-e", "false = Code.ensure_loaded?(Rustler); Consumer.view()"], consumer, env)
    refute mix!(["compile"], consumer, env) =~ "Compiling"

    native = Path.join(package, "mix/native.exs")
    File.write!(native, File.read!(native) <> "\n# Changed build input\n")
    assert mix!(["compile"], consumer, env) =~ "Compiling"

    targets = Path.join(package, "mix/targets.exs")

    File.write!(
      targets,
      String.replace(
        File.read!(targets),
        "%{",
        "%{\n  \"probe-nerves-linux-gnu\" => \"x86_64-unknown-linux-gnu\",",
        global: false
      )
    )

    assert mix!(["compile"], consumer, env) =~ "Compiling"

    mix!(
      [
        "run",
        "-e",
        """
        for mod <- [Emerge.Mix.Native, Emerge.Mix.Package, Emerge.Mix.Docs] do
          :code.purge(mod)
          :code.delete(mod)
        end
        [:drm] = EmergeSkia.BuildConfig.default_compiled_backends(%{"CC" => "probe-nerves-linux-gnu-gcc"})
        false = Code.ensure_loaded?(Emerge.Mix.Native)
        """
      ],
      consumer,
      env
    )
  end

  defp mix!(args, root, env) do
    {output, status} =
      System.cmd(System.find_executable("mix"), args, cd: root, env: env, stderr_to_stdout: true)

    assert status == 0, output
    refute output =~ "redefining module", output
    output
  end
end
