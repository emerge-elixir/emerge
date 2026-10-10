defmodule EmergeSkia.ConfigControlTest do
  use ExUnit.Case, async: false
  alias Emerge.Config
  alias EmergeSkia.Native

  setup do
    config =
      Emerge.Config.Normalize.config!(%Config{
        output: %Config.Headless{target: self(), size: {2, 2}, frame_message: "config_frame"},
        renderer: [api: :raster],
        assets: [otp_app: :emerge]
      })

    {:ok, renderer} = EmergeSkia.start(config)
    on_exit(fn -> EmergeSkia.stop(renderer) end)
    %{renderer: renderer, config: config}
  end

  test "native completion and status identify the session without requiring a tree", context do
    %{renderer: renderer, config: %Config{input: %Config.Input{} = input} = config} = context
    assert {:ok, initial} = EmergeSkia.config_status(renderer)
    assert initial.desired_revision == 0
    assert initial.applied_revision == nil
    assert initial.pending_revision == nil
    assert {:ok, %{session_id: session}} = EmergeSkia.renderer_status(renderer)
    assert initial.session_id == session

    input = %Config.Input{input | events: [:key]}
    desired = %Config{config | input: input}
    assert {:ok, {^session, 1} = token} = EmergeSkia.configure(renderer, config, desired)
    assert_receive {:emerge_skia_config, ^token, :ok}, 5_000
    assert {:ok, status} = EmergeSkia.config_status(renderer)
    assert status.desired_revision == 1
    assert status.applied_revision == 1
    assert status.pending_revision == nil
    assert status.error == nil
    refute_receive {"config_frame", _}, 50
  end

  test "invalid or unsupported controls do not enter native desired state", %{renderer: renderer} do
    for invalid <- [
          %{title: nil, target_fps: 0, input_mask: 1},
          %{title: nil, target_fps: nil, input_mask: 256},
          %{title: "not a window", target_fps: nil, input_mask: 1}
        ] do
      assert {:error, _} = Native.configure(renderer, invalid)
    end

    assert {:ok, %{desired_revision: 0, pending_revision: nil}} =
             EmergeSkia.config_status(renderer)

    refute_receive {:emerge_skia_config, _, _}, 50
  end

  test "bursts report superseded work and apply the final accepted revision", %{
    renderer: renderer
  } do
    tokens =
      Enum.flat_map(1..200, fn n ->
        case Native.configure(renderer, %{title: nil, target_fps: nil, input_mask: rem(n, 2)}) do
          {:ok, token} -> [token]
          {:error, "renderer control queue is full"} -> []
        end
      end)

    assert tokens != []
    await_completions(MapSet.new(tokens))
    {session, revision} = List.last(tokens)
    assert {:ok, status} = EmergeSkia.config_status(renderer)
    assert status.session_id == session
    assert status.desired_revision == revision
    assert status.applied_revision == revision
    assert status.pending_revision == nil
    assert status.failed_revision == nil
    refute_receive {:emerge_skia_config, _, _}, 20
    refute_receive {"config_frame", _}, 20
  end

  test "stopped resources reject revisions and new sessions have distinct tokens", context do
    %{renderer: renderer, config: config} = context
    assert :ok = EmergeSkia.stop(renderer)
    assert {:error, _} = EmergeSkia.configure(renderer, config, config)
    assert {:ok, old} = EmergeSkia.config_status(renderer)
    assert old.desired_revision == 0

    {:ok, replacement} = EmergeSkia.start(config)
    on_exit(fn -> EmergeSkia.stop(replacement) end)
    assert {:ok, {session, 1} = token} = EmergeSkia.configure(replacement, config, config)
    assert session != old.session_id
    assert_receive {:emerge_skia_config, ^token, :ok}, 5_000
  end

  defp await_completions(tokens) do
    if MapSet.size(tokens) > 0 do
      receive do
        {:emerge_skia_config, token, result} ->
          assert MapSet.member?(tokens, token)
          assert result in [:ok, {:error, "config superseded"}]
          await_completions(MapSet.delete(tokens, token))
      after
        5_000 -> flunk("missing config completions: #{inspect(tokens)}")
      end
    end
  end
end
