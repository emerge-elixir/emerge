defmodule EmergeSkia.BuildConfigTest do
  use ExUnit.Case, async: true

  alias EmergeSkia.BuildConfig

  test "normalize_compiled_backends! defaults to canonical backend order" do
    assert BuildConfig.normalize_compiled_backends!([:drm, :wayland, :drm]) == [:wayland, :drm]
  end

  test "compiled_backends_to_rustler_features returns stable feature order" do
    assert BuildConfig.compiled_backends_to_rustler_features([:drm, :wayland]) == [
             "wayland",
             "drm"
           ]
  end

  test "normalize_compiled_backends! accepts an empty backend list" do
    assert BuildConfig.normalize_compiled_backends!([]) == []
    assert BuildConfig.compiled_backends_to_rustler_features([]) == []
  end

  test "normalize_compiled_backends! rejects invalid config shapes" do
    assert_raise ArgumentError, ~r/compiled_backends: .*must be a list of backend atoms/, fn ->
      BuildConfig.normalize_compiled_backends!(:wayland)
    end
  end

  test "normalize_compiled_backends! rejects invalid entries" do
    assert_raise ArgumentError, ~r/containing only :wayland and :drm/, fn ->
      BuildConfig.normalize_compiled_backends!([:wayland, :bogus, "drm"])
    end
  end
end
