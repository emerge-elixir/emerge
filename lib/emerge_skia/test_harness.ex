defmodule EmergeSkia.TestHarness do
  @moduledoc false

  alias EmergeSkia.Native

  @spec new(pos_integer(), pos_integer()) :: reference()
  def new(width, height) do
    case Native.test_harness_new(width, height) do
      {:ok, harness} -> harness
      {:error, reason} -> raise "test_harness_new failed: #{reason}"
      harness -> harness
    end
  end

  @spec upload_full_bin(reference(), binary()) :: :ok | {:error, String.t()}
  def upload_full_bin(harness, full_bin),
    do: unwrap_ok(Native.test_harness_upload(harness, full_bin))

  @spec apply_patch_bin(reference(), binary()) :: :ok | {:error, String.t()}
  def apply_patch_bin(harness, patch_bin),
    do: unwrap_ok(Native.test_harness_patch(harness, patch_bin))

  @spec cursor_pos(reference(), number(), number()) :: :ok | {:error, String.t()}
  def cursor_pos(harness, x, y), do: unwrap_ok(Native.test_harness_cursor_pos(harness, x, y))

  @spec animation_pulse(reference(), non_neg_integer(), non_neg_integer()) ::
          :ok | {:error, String.t()}
  def animation_pulse(harness, presented_ms, predicted_ms) do
    unwrap_ok(Native.test_harness_animation_pulse(harness, presented_ms, predicted_ms))
  end

  @spec reset_clock(reference()) :: :ok
  def reset_clock(harness), do: unwrap_ok(Native.test_harness_reset_clock(harness))

  @spec await_render(reference(), non_neg_integer()) :: :ok | {:error, String.t()}
  def await_render(harness, timeout_ms \\ 250),
    do: unwrap_ok(Native.test_harness_await_render(harness, timeout_ms))

  @spec drain_mouse_over_msgs(reference(), non_neg_integer()) :: [{binary(), boolean()}]
  def drain_mouse_over_msgs(harness, timeout_ms \\ 20),
    do: Native.test_harness_drain_mouse_over_msgs(harness, timeout_ms)

  @spec stop(reference()) :: :ok
  def stop(harness), do: unwrap_ok(Native.test_harness_stop(harness))

  defp unwrap_ok(:ok), do: :ok
  defp unwrap_ok({:ok, :ok}), do: :ok
  defp unwrap_ok(other), do: other
end
