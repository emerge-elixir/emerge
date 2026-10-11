defmodule Emerge.Config.Headless do
  @moduledoc """
  Offscreen output to a live local process. Supply `target` explicitly: its `nil`
  default is not valid at startup. `frames` requires an explicit
  `Emerge.Config.BinaryFrames` or `Emerge.Config.PrimeFrames` variant.

  Frames arrive as `{frame_message, %VideoInterop.Frame{}}`. Binary output owns its
  storage; PRIME output has a lease the recipient must retire. `size` is the output
  buffer size. The drawing cap lives in `Emerge.Config.Renderer.target_fps`, not
  this record. Output fields are startup-only. Headless has no physical input.

  ## Fields and defaults

  | Field | Default |
  |---|---|
  | `size` | `{800, 600}` |
  | `target` | `nil` |
  | `frame_message` | `:emerge_skia_frame` |
  | `frames` | `%Emerge.Config.BinaryFrames{}` |
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
