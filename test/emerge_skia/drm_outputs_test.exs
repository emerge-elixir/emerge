defmodule EmergeSkia.DrmOutputsTest do
  use ExUnit.Case, async: true
  alias EmergeSkia.Options

  test "discovery failures are tagged and need no renderer or otp_app" do
    assert {:error, reason} = EmergeSkia.drm_outputs(drm_card: "/does/not/exist/emerge-drm")
    assert is_binary(reason)
  end

  test "discovery validates its options" do
    assert_raise ArgumentError, fn -> EmergeSkia.drm_outputs(drm_card: 3) end
    assert_raise ArgumentError, fn -> EmergeSkia.drm_outputs(backend: :drm) end
    assert_raise ArgumentError, fn -> EmergeSkia.drm_outputs(:drm) end
  end

  test "output selection passes exact advertised mode IDs and retains automatic defaults" do
    assert %{drm_output: nil, drm_mode: nil, owner: nil} = Options.build_start_native_opts!([])

    assert %{drm_output: "DP-1", drm_mode: "exact-timings"} =
             Options.build_start_native_opts!(
               backend: :drm,
               drm_output: "DP-1",
               drm_mode: %{id: "exact-timings", width: 1920}
             )

    assert %{drm_mode: "exact-timings"} =
             Options.build_start_native_opts!(
               backend: :drm,
               drm_output: "DP-1",
               drm_mode: "exact-timings"
             )
  end

  test "rejects ambiguous modes and selectors on another backend" do
    for opts <- [
          [backend: :drm, drm_mode: "id"],
          [backend: :wayland, drm_output: "DP-1"],
          [backend: :drm, drm_output: ""],
          [backend: :drm, drm_output: 2],
          [backend: :drm, drm_output: "DP-1", drm_mode: {1920, 1080}]
        ] do
      assert_raise ArgumentError, fn -> Options.build_start_native_opts!(opts) end
    end
  end
end
