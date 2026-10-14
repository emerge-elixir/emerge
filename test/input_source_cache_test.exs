Code.require_file("../mix/input_source_cache.exs", __DIR__)

defmodule Emerge.InputSourceCacheTest do
  use ExUnit.Case, async: true

  alias Emerge.Mix.InputSourceCache

  @manifest Path.expand("../native/emerge_input/sources.rs", __DIR__)

  @tag :tmp_dir
  test "only a complete verified cache is eligible for saving", %{tmp_dir: cache} do
    hash = :crypto.hash(:sha256, "abc") |> Base.encode16(case: :lower)
    pins = [{"first.tar.gz", hash}, {"second.tar.xz", hash}]
    refute InputSourceCache.complete?(cache, [])
    refute InputSourceCache.complete?(cache, pins)
    File.write!(Path.join(cache, "first.tar.gz"), "abc")
    refute InputSourceCache.complete?(cache, pins)
    File.write!(Path.join(cache, "second.tar.xz"), "bad")
    refute InputSourceCache.complete?(cache, pins)
    File.write!(Path.join(cache, "second.tar.xz"), "abc")
    assert InputSourceCache.complete?(cache, pins)
  end

  test "cache verification uses the authoritative Rust pins, not duplicated versions" do
    pins = @manifest |> File.read!() |> InputSourceCache.pins!()
    assert length(pins) == 4

    assert {"libevdev-1.13.4.tar.xz",
            "f00ab8d42ad8b905296fab67e13b871f1a424839331516642100f82ad88127cd"} in pins
  end

  test "incomplete source pin parsing fails closed" do
    manifest = File.read!(@manifest)
    malformed = String.replace(manifest, "f00ab8d42ad8b905", "not-a-sha256-pin")
    assert_raise ArgumentError, fn -> InputSourceCache.pins!(malformed) end
    unsafe = String.replace(manifest, "libevdev-1.13.4.tar.xz\"", "../unsafe.tar.xz\"")
    assert_raise ArgumentError, fn -> InputSourceCache.pins!(unsafe) end
  end
end
