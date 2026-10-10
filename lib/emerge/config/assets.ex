defmodule Emerge.Config.Assets do
  @moduledoc """
  Application assets, fonts, runtime filesystem access and decoding policy. `otp_app` is inferred from the viewport module when omitted.

  ## Fields and defaults

  | Field | Default |
  |---|---|
  | `otp_app` | `nil` |
  | `fonts` | `[]` |
  | `decode_at_size` | `false` |
  | `runtime_paths` | `%Emerge.Config.RuntimePaths{}` |
  | `cache` | `%Emerge.Config.AssetCache{}` |
  """
  defstruct otp_app: nil,
            fonts: [],
            decode_at_size: false,
            runtime_paths: %Emerge.Config.RuntimePaths{},
            cache: %Emerge.Config.AssetCache{}

  @type t :: %__MODULE__{
          otp_app: atom() | nil,
          fonts: [Emerge.Config.Font.t() | Emerge.Config.Font.options()],
          decode_at_size: boolean(),
          runtime_paths: Emerge.Config.RuntimePaths.t() | Emerge.Config.RuntimePaths.options(),
          cache: Emerge.Config.AssetCache.t() | Emerge.Config.AssetCache.options()
        }
  @typedoc "Typed keyword shorthand accepted in this record's parent field."
  @type options :: [
          {:otp_app, atom() | nil}
          | {:fonts, [Emerge.Config.Font.t() | Emerge.Config.Font.options()]}
          | {:decode_at_size, boolean()}
          | {:runtime_paths,
             Emerge.Config.RuntimePaths.t() | Emerge.Config.RuntimePaths.options()}
          | {:cache, Emerge.Config.AssetCache.t() | Emerge.Config.AssetCache.options()}
        ]
  @typedoc "Canonical form after nested keyword records have been expanded."
  @type normalized_t :: %__MODULE__{
          otp_app: atom() | nil,
          fonts: [Emerge.Config.Font.normalized_t()],
          decode_at_size: boolean(),
          runtime_paths: Emerge.Config.RuntimePaths.normalized_t(),
          cache: Emerge.Config.AssetCache.normalized_t()
        }

  @doc false
  def __schema__,
    do: [
      otp_app: {:optional, :module},
      fonts: {:list, {:record, Emerge.Config.Font}},
      decode_at_size: :boolean,
      runtime_paths: {:record, Emerge.Config.RuntimePaths},
      cache: {:record, Emerge.Config.AssetCache}
    ]
end
