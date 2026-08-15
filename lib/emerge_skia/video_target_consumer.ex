defmodule EmergeSkia.VideoTargetConsumer do
  @moduledoc false

  alias EmergeSkia.{VideoConsumerSession, VideoTarget}
  alias VideoInterop.DMABuf
  alias VideoInterop.Format

  @nv12 VideoInterop.DMABuf.FourCC.from_string!("NV12")
  @abgr8888 VideoInterop.DMABuf.FourCC.from_string!("AB24")
  @xrgb8888 VideoInterop.DMABuf.FourCC.from_string!("XR24")

  @spec open(VideoTarget.t(), Format.t(), keyword()) ::
          {:ok, VideoConsumerSession.t()} | {:error, term()}
  def open(%VideoTarget{} = target, %Format{} = format, opts) do
    with :ok <- validate_target_format(target, format),
         owner when is_pid(owner) <- Keyword.fetch!(opts, :owner) do
      VideoConsumerSession.open(target, format, owner)
    else
      {:error, _reason} = error -> error
    end
  end

  @doc false
  @spec validate_target_format(VideoTarget.t(), Format.t()) :: :ok | {:error, term()}
  def validate_target_format(
        %VideoTarget{mode: :prime, width: width, height: height},
        %Format{
          width: width,
          height: height,
          storage: %DMABuf.Format{fourcc: fourcc},
          interlace_mode: :progressive,
          alpha_mode: alpha_mode
        }
      )
      when fourcc in [@nv12, @abgr8888, @xrgb8888] do
    cond do
      fourcc in [@nv12, @xrgb8888] and alpha_mode != :opaque ->
        {:error, {:unsupported_alpha_mode, alpha_mode}}

      fourcc == @abgr8888 and alpha_mode not in [:opaque, :premultiplied] ->
        {:error, {:unsupported_alpha_mode, alpha_mode}}

      true ->
        :ok
    end
  end

  def validate_target_format(%VideoTarget{mode: mode}, _format) when mode != :prime,
    do: {:error, {:wrong_mode, mode}}

  def validate_target_format(%VideoTarget{width: width, height: height}, %Format{
        width: actual_width,
        height: actual_height
      })
      when width != actual_width or height != actual_height,
      do: {:error, {:wrong_size, {actual_width, actual_height}, {width, height}}}

  def validate_target_format(_target, %Format{interlace_mode: mode})
      when mode != :progressive,
      do: {:error, {:unsupported_interlace_mode, mode}}

  def validate_target_format(_target, %Format{storage: %DMABuf.Format{fourcc: fourcc}}),
    do: {:error, {:unsupported_fourcc, fourcc}}

  def validate_target_format(_target, _format), do: {:error, :unsupported_storage}
end

defimpl VideoInterop.Consumer, for: EmergeSkia.VideoTarget do
  def open(target, format, opts), do: EmergeSkia.VideoTargetConsumer.open(target, format, opts)
end
