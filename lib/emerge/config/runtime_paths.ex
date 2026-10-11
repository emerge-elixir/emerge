defmodule Emerge.Config.RuntimePaths do
  @moduledoc """
  Allow images from outside your application's `priv/` directory. Set this record
  as `Emerge.Config.Assets.runtime_paths`. This controls runtime image paths,
  not your application's general filesystem access.

  ```elixir
  %Emerge.Config{
    assets: %Emerge.Config.Assets{
      runtime_paths: %Emerge.Config.RuntimePaths{
        enabled: true,
        allowlist: ["/data/photos"]
      }
    }
  }
  ```

  ## Fields

  - `enabled` — `false` by default. Set `true` to allow runtime sources such as
    `{:path, "/data/photos/photo.jpg"}`; an allowlist is also required.
  - `allowlist` — allowed absolute directory roots; defaults to `[]`, allowing
    no runtime files. Keep the list as narrow as possible.
  - `follow_symlinks` — `false` by default, rejecting symlink path components.
    With `true`, resolved paths must still fall within an allowed directory.
  - `max_file_size` — positive encoded-file size limit in bytes; defaults to
    `25_000_000`. This does not limit decoded image dimensions or peak memory.
  - `extensions` — allowed filename extensions, including their leading dot.
    Defaults to `[".png", ".jpg", ".jpeg", ".webp", ".gif", ".bmp", ".svg"]`.
    The list replaces these defaults when supplied.

  Logical application assets do not need this opt-in. All fields require a new
  viewport to change. See [Use assets](use_assets.html) for source syntax.
  """
  defstruct enabled: false,
            allowlist: [],
            follow_symlinks: false,
            max_file_size: 25_000_000,
            extensions: [".png", ".jpg", ".jpeg", ".webp", ".gif", ".bmp", ".svg"]

  @type t :: %__MODULE__{
          enabled: boolean(),
          allowlist: [String.t()],
          follow_symlinks: boolean(),
          max_file_size: pos_integer(),
          extensions: [String.t()]
        }
  @typedoc "Typed keyword shorthand accepted in this record's parent field."
  @type options :: [
          {:enabled, boolean()}
          | {:allowlist, [String.t()]}
          | {:follow_symlinks, boolean()}
          | {:max_file_size, pos_integer()}
          | {:extensions, [String.t()]}
        ]
  @typedoc "Canonical form after nested keyword records have been expanded."
  @type normalized_t :: t()

  @doc false
  def __schema__,
    do: [
      enabled: :boolean,
      allowlist: {:list, :string},
      follow_symlinks: :boolean,
      max_file_size: :positive_integer,
      extensions: {:list, :string}
    ]
end
