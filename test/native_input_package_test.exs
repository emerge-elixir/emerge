defmodule Emerge.NativeInputPackageTest do
  use ExUnit.Case, async: true

  test "the source package retains the input downloader, integration and licenses" do
    root = Path.expand("..", __DIR__)
    package_files = Emerge.Mix.Package.config(root, "test")[:files]

    required =
      ~w(Cargo.toml Cargo.lock build.rs sources.rs event_names.rs shim.c classification-test.c quirks-bundle.h README.md)
      |> Enum.map(&Path.join("native/emerge_input", &1))

    required =
      required ++
        (Path.wildcard(Path.join(root, "native/emerge_input/{src,licenses,tests,examples}/**/*"))
         |> Enum.filter(&File.regular?/1)
         |> Enum.map(&Path.relative_to(&1, root)))

    for file <- required, do: assert(file in package_files, "Missing bundled input file: #{file}")

    for license <- ~w(libinput-MIT.txt libevdev-MIT.txt mtdev-MIT.txt libudev-zero-ISC.txt) do
      assert "native/emerge_input/licenses/#{license}" in package_files
    end

    refute File.dir?(Path.join(root, "native/emerge_input/vendor"))
    refute File.dir?(Path.join(root, "priv/libinput"))

    refute Enum.any?(package_files, fn file ->
             String.contains?(file, "emerge_input/target") or
               String.contains?(file, "emerge_input/vendor") or
               String.starts_with?(file, "priv/libinput")
           end)
  end
end
