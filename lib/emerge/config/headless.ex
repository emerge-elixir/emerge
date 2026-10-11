defmodule Emerge.Config.Headless do
  @moduledoc """
  Draw offscreen and send frames to an Elixir process. Set this record as
  `Emerge.Config.output`.

  ```elixir
  %Emerge.Config{
    output: %Emerge.Config.Headless{target: self(), size: {400, 300}},
    renderer: %Emerge.Config.Renderer{api: :raster},
    assets: %Emerge.Config.Assets{otp_app: :my_app}
  }
  ```

  ## Fields

  - `size` — output `{width, height}` in positive integer pixels;
    defaults to `{800, 600}`.
  - `target` — the live local PID receiving frames. You must supply it;
    the default `nil` is not valid when starting a headless renderer.
  - `frame_message` — message tag, an atom or string;
    defaults to `:emerge_skia_frame`.
  - `frames` — `%Emerge.Config.BinaryFrames{}` (default) for owned pixel data,
    or `%Emerge.Config.PrimeFrames{}` for Linux DMA-BUF output. Use an explicit
    struct to choose the format.

  The target receives `{frame_message, %VideoInterop.Frame{}}`. Binary frames
  need no release; PRIME frames have a lease the recipient must release.
  Headless output has no physical keyboard or pointer input.

  ## Changing settings

  These fields require a new viewport. The frame-rate limit lives in
  `Emerge.Config.Renderer.target_fps` and can change through `config`.
  """
  defstruct size: {800, 600},
            target: nil,
            frame_message: :emerge_skia_frame,
            frames: %Emerge.Config.BinaryFrames{}

  @type t :: %__MODULE__{
          size: {pos_integer(), pos_integer()},
          target: pid() | nil,
          frame_message: atom() | String.t(),
          frames: Emerge.Config.BinaryFrames.t() | Emerge.Config.PrimeFrames.t()
        }
  @typedoc "Canonical form after nested keyword records have been expanded."
  @type normalized_t :: %__MODULE__{
          size: {pos_integer(), pos_integer()},
          target: pid() | nil,
          frame_message: atom() | String.t(),
          frames:
            Emerge.Config.BinaryFrames.normalized_t() | Emerge.Config.PrimeFrames.normalized_t()
        }

  @doc false
  def __schema__,
    do: [
      size: :size,
      target: {:optional, :local_pid},
      frame_message: :message_tag,
      frames: {:variants, [Emerge.Config.BinaryFrames, Emerge.Config.PrimeFrames]}
    ]
end
