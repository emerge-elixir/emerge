defmodule Emerge.Assets.PathTest do
  use ExUnit.Case, async: false

  alias Emerge.Assets.Ref

  setup_all do
    priv_dir =
      case :code.priv_dir(:emerge) do
        path when is_list(path) -> List.to_string(path)
        _ -> raise "failed to resolve :emerge priv dir for tests"
      end

    fixture_dir = Path.join(priv_dir, "test_assets")
    fixture_path = Path.join(fixture_dir, "path_test_asset.txt")

    File.mkdir_p!(fixture_dir)
    File.write!(fixture_path, "asset")

    on_exit(fn ->
      File.rm(fixture_path)
    end)

    {:ok, logical_path: "test_assets/path_test_asset.txt"}
  end

  test "~m verifies files inside otp_app priv", %{logical_path: logical_path} do
    module = unique_module("Valid")

    code = """
    defmodule #{inspect(module)} do
      use Emerge.Assets.Path, otp_app: :emerge
      def ref, do: ~m\"#{logical_path}\"
    end
    """

    [{^module, _}] = Code.compile_string(code)

    assert %Ref{path: ^logical_path, verified?: true} = apply(module, :ref, [])
  end

  test "use Emerge.Assets.Path requires otp_app option" do
    module = unique_module("MissingOtpApp")

    code = """
    defmodule #{inspect(module)} do
      use Emerge.Assets.Path
      def ref, do: ~m\"images/logo.png\"
    end
    """

    assert_raise ArgumentError, ~r/requires otp_app/, fn ->
      Code.compile_string(code)
    end
  end

  test "~m rejects parent directory traversal" do
    module = unique_module("Traversal")

    code = """
    defmodule #{inspect(module)} do
      use Emerge.Assets.Path, otp_app: :emerge
      def ref, do: ~m\"../escape.png\"
    end
    """

    assert_raise ArgumentError, ~r/may not contain '\.\.'/, fn ->
      Code.compile_string(code)
    end
  end

  defp unique_module(tag) do
    Module.concat(["Emerge", "Assets", "PathTest", "#{tag}#{System.unique_integer([:positive])}"])
  end
end
