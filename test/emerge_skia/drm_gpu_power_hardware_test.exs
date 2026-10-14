defmodule EmergeSkia.DrmGpuPowerHardwareTest do
  use ExUnit.Case, async: false
  use Emerge.UI
  @moduletag :drm_hardware
  @moduletag capture_log: true
  @moduletag timeout: 120_000

  setup_all do
    card = System.fetch_env!("EMERGE_DRM_TEST_CARD")
    connector = System.fetch_env!("EMERGE_DRM_TEST_CONNECTOR")

    renderer =
      case System.get_env("EMERGE_DRM_TEST_POWER_RENDERER", "vulkan") do
        "vulkan" -> [api: :vulkan, device: System.fetch_env!("EMERGE_DRM_TEST_VULKAN_NODE")]
        "gpu_upload" -> [api: :raster, raster_present: :gpu_upload]
        other -> raise ArgumentError, "unsupported DRM power test renderer: #{inspect(other)}"
      end

    backlight = System.get_env("EMERGE_DRM_TEST_BACKLIGHT")
    {:ok, outputs} = EmergeSkia.drm_outputs(drm_card: card)
    output = Enum.find(outputs, &(&1.name == connector and &1.connected and &1.modes != []))
    assert output, "the explicitly selected connector must be connected and advertise modes"
    mode = Enum.find(output.modes, & &1.preferred) || hd(output.modes)

    config =
      Emerge.Config.Normalize.config!(%Emerge.Config{
        output: %Emerge.Config.DRM{
          card: card,
          connector: connector,
          mode: mode,
          display: :off,
          backlight: [device: backlight, brightness: nil]
        },
        renderer: renderer,
        assets: [otp_app: :emerge],
        diagnostics: [stats: true],
        runtime: [startup_retries: 0]
      })

    {real_card, 0} = System.cmd("readlink", ["-f", card])
    kms_status = "/sys/class/drm/#{Path.basename(String.trim(real_card))}-#{connector}/enabled"
    assert File.exists?(kms_status)

    %{
      config: config,
      managed?: not is_nil(backlight),
      kms_status: kms_status,
      size: {mode.width, mode.height}
    }
  end

  test "initial-off, delayed upload and repeated power/brightness changes retain the owner", %{
    config: initial,
    managed?: managed?,
    kms_status: kms_status,
    size: size
  } do
    {:ok, renderer} = EmergeSkia.start(initial)

    try do
      api = initial.renderer.api

      assert {:ok, %{rendering_api: %{selected: ^api}} = info} =
               EmergeSkia.renderer_info(renderer)

      case api do
        :vulkan ->
          assert info.vulkan_device.software == false

        :raster ->
          assert info.capabilities.gpu == false
          assert info.capabilities.prime_video == false
          assert :gpu_upload in info.capabilities.raster_present
          assert_receive {:emerge_skia_log, :info, "drm", "GL: " <> report}, 1_000
          refute report =~ ~r/llvmpipe|softpipe|software rasterizer/i
          IO.puts("GPU-upload hardware GL: #{report}")
      end

      IO.puts("DRM power test renderer: #{inspect(info, pretty: true, limit: :infinity)}")

      assert frames(renderer) == 0
      assert_kms(kms_status, "disabled")
      on = power(initial, :on, nil)
      {:ok, token} = EmergeSkia.configure(renderer, initial, on)
      refute_receive {:emerge_skia_config, ^token, :ok}, 100
      assert frames(renderer) == 0

      EmergeSkia.upload_tree(
        renderer,
        application_tree(size, "First GPU-presented application frame", {30, 80, 220})
      )

      await_config(token)
      assert frames(renderer) > 0
      assert_kms(kms_status, "enabled")
      assert_capture(renderer, size, {30, 80, 220})

      levels = if managed?, do: [0.0, 0.5, 1.0, nil], else: [nil]

      final =
        Enum.reduce(1..3, on, fn cycle, on ->
          Enum.reduce(levels, on, fn brightness, on ->
            before = frames(renderer)
            dimmed = power(on, :on, brightness)
            apply_config(renderer, on, dimmed)
            Process.sleep(50)
            assert frames(renderer) == before, "brightness alone must not redraw"

            off = power(dimmed, :off, brightness)
            apply_config(renderer, dimmed, off)
            assert_kms(kms_status, "disabled")
            before = frames(renderer)
            Process.sleep(100)
            assert frames(renderer) == before, "off must not present routine frames"
            assert EmergeSkia.running?(renderer)

            dark_update = power(off, :off, nil)
            apply_config(renderer, off, dark_update)
            assert frames(renderer) == before
            resumed = power(dark_update, :on, nil)
            apply_config(renderer, dark_update, resumed)
            assert frames(renderer) > before
            assert_kms(kms_status, "enabled")
            color = {40 * cycle, 180, 40}

            EmergeSkia.upload_tree(
              renderer,
              application_tree(size, "GPU power cycle #{cycle}", color)
            )

            assert eventually(fn -> frames(renderer) > before + 1 end)
            assert_capture(renderer, size, color)
            resumed
          end)
        end)

      apply_config(renderer, final, power(final, :off, nil))
    after
      assert :ok = EmergeSkia.stop(renderer, timeout: 5_000)
      print_native_logs()
    end

    assert {:ok, %{cleanup_complete: true}} = EmergeSkia.renderer_status(renderer)
    assert_kms(kms_status, "disabled")
    # A clean stop permits reacquisition; it must not have entered quarantine.
    {:ok, replacement} = EmergeSkia.start(initial)
    assert :ok = EmergeSkia.stop(replacement, timeout: 5_000)
    assert {:ok, %{cleanup_complete: true}} = EmergeSkia.renderer_status(replacement)
    print_native_logs()
  end

  test "Stop before first upload works both initially on and off", %{config: config} do
    for display <- [:on, :off] do
      {:ok, renderer} = EmergeSkia.start(power(config, display, nil))
      assert frames(renderer) == 0
      assert :ok = EmergeSkia.stop(renderer, timeout: 5_000)
      assert {:ok, %{cleanup_complete: true}} = EmergeSkia.renderer_status(renderer)
      print_native_logs()
    end
  end

  test "rapid on/off supersession keeps the owner dark with bounded descriptors", %{
    config: initial,
    kms_status: kms_status,
    size: size
  } do
    on = power(initial, :on, nil)
    {:ok, renderer} = EmergeSkia.start(on)

    try do
      EmergeSkia.upload_tree(
        renderer,
        application_tree(size, "Rapid GPU power changes", {180, 40, 80})
      )

      assert eventually(fn -> frames(renderer) > 0 end)
      assert_capture(renderer, size, {180, 40, 80})
      apply_config(renderer, on, initial)
      baseline_fds = length(File.ls!("/proc/self/fd"))

      Enum.each(1..20, fn _ ->
        {:ok, waking} = EmergeSkia.configure(renderer, initial, on)
        {:ok, darkening} = EmergeSkia.configure(renderer, on, initial)
        await_config(darkening)
        assert_receive {:emerge_skia_config, ^waking, result}, 10_000
        assert result in [:ok, {:error, "config superseded"}]
        assert EmergeSkia.running?(renderer)
        assert_kms(kms_status, "disabled")
      end)

      before = frames(renderer)
      Process.sleep(1_000)
      assert frames(renderer) == before
      final_fds = length(File.ls!("/proc/self/fd"))

      IO.puts(
        "DRM #{initial.renderer.api} off descriptor count: baseline=#{baseline_fds} final=#{final_fds}"
      )

      assert final_fds <= baseline_fds
      apply_config(renderer, initial, on)
      assert_capture(renderer, size, {180, 40, 80})
    after
      assert :ok = EmergeSkia.stop(renderer, timeout: 5_000)
      print_native_logs()
    end

    assert {:ok, %{cleanup_complete: true}} = EmergeSkia.renderer_status(renderer)
    assert_kms(kms_status, "disabled")
  end

  defp application_tree({width, height}, label, {r, g, b}) do
    el(
      [
        width(px(width)),
        height(px(height)),
        Background.color(Emerge.UI.Color.color_rgb(r, g, b)),
        Font.color(Emerge.UI.Color.color_rgb(255, 255, 255)),
        Font.size(48)
      ],
      text(label)
    )
  end

  defp assert_capture(renderer, {width, height}, {r, g, b}) do
    assert {:ok, pixels} = EmergeSkia.render_to_pixels(renderer)
    assert byte_size(pixels) == width * height * 4
    assert binary_part(pixels, byte_size(pixels) - 4, 4) == <<r, g, b, 255>>
  end

  defp assert_kms(path, expected) do
    assert String.trim(File.read!(path)) == expected
    IO.puts("DRM KMS #{Path.dirname(path)}: #{expected}")
  end

  defp print_native_logs do
    receive do
      {:emerge_skia_log, level, backend, message} ->
        IO.puts("Native #{level} #{backend}: #{message}")
        print_native_logs()
    after
      0 -> :ok
    end
  end

  defp power(config, display, brightness) do
    backlight = %{config.output.backlight | brightness: brightness}
    %{config | output: %{config.output | display: display, backlight: backlight}}
  end

  defp apply_config(renderer, previous, config) do
    {:ok, token} = EmergeSkia.configure(renderer, previous, config)
    await_config(token)
    {:ok, status} = EmergeSkia.config_status(renderer)
    assert status.applied_revision == elem(token, 1)
    assert status.pending_revision == nil
  end

  defp await_config(token), do: assert_receive({:emerge_skia_config, ^token, :ok}, 10_000)

  defp frames(renderer) do
    {:ok, %{frames: %{frame_count: count}}} = EmergeSkia.stats(renderer, :peek)
    count
  end

  defp eventually(fun, attempts \\ 250)
  defp eventually(fun, 0), do: fun.()

  defp eventually(fun, attempts) do
    if fun.(),
      do: true,
      else:
        (
          Process.sleep(20)
          eventually(fun, attempts - 1)
        )
  end
end
