defmodule Emerge.Config.Assets do
  @moduledoc """
  Fonts, images and filesystem access for one renderer. Set this record as
  `Emerge.Config.assets`.

  ```elixir
  %Emerge.Config{
    assets: %Emerge.Config.Assets{
      otp_app: :my_app,
      fonts: [%Emerge.Config.Font{family: "Inter", source: "fonts/Inter-Regular.ttf"}]
    }
  }
  ```

  ## Fields

  - `otp_app` — OTP application whose `priv/` directory contains logical asset
    paths. Defaults to `nil`: a viewport infers its application where possible,
    but standalone `EmergeSkia.start/1` callers must supply it.
  - `fonts` — list of `Emerge.Config.Font` registrations; defaults to `[]`.
    Register each family/weight/italic variant your UI uses.
  - `decode_at_size` — `false` by default. With `true`, raster images are decoded
    near their drawn size rather than always retaining the source dimensions.
    Useful for thumbnails of large images; larger targets can request a larger decode.
  - `runtime_paths` — `Emerge.Config.RuntimePaths`. Its default disables images
    loaded from absolute filesystem paths.
  - `cache` — `Emerge.Config.AssetCache`, limiting retained image pixels and parsed
    SVGs. These limits are separate from renderer paint caching.

  All fields require a new viewport to change. Lists such as `fonts` replace the
  previous list rather than append to it. See [Use assets](use_assets.html) for
  paths, font usage and memory-tuning examples.
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
