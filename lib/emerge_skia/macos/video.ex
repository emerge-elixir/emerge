defmodule EmergeSkia.Macos.Video do
  @moduledoc false

  def encode(
        target,
        %VideoInterop.Frame{
          storage: %VideoInterop.Binary{data: data, planes: [plane]},
          format: %VideoInterop.Format{
            storage: %VideoInterop.Binary.Format{pixel_format: :rgba8888},
            alpha_mode: alpha
          },
          visible_rect: rect,
          lease: nil,
          acquire_sync: :implicit
        } = frame
      )
      when is_atom(target) do
    with :ok <- VideoInterop.validate(frame),
         true <- alpha in [:premultiplied, :straight, :opaque],
         name = Atom.to_string(target),
         true <- byte_size(name) > 0 and byte_size(name) <= 65_535,
         true <- rect.width * rect.height * 4 + byte_size(name) + 11 <= 64 * 1024 * 1024 do
      alpha_tag = Enum.find_index([:premultiplied, :straight, :opaque], &(&1 == alpha))

      rows =
        Enum.map(0..(rect.height - 1), fn y ->
          binary_part(
            data,
            plane.offset + (rect.y + y) * plane.stride + rect.x * 4,
            rect.width * 4
          )
        end)

      {:ok,
       IO.iodata_to_binary([
         <<byte_size(name)::16, name::binary, rect.width::32, rect.height::32, alpha_tag>>,
         rows
       ])}
    else
      {:error, reason} -> {:error, reason}
      false -> {:error, :unsupported_binary_video_frame}
    end
  end

  def encode(_target, _frame), do: {:error, :video_submission_unsupported}
end
