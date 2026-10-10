defmodule Emerge.Config.RuntimePaths do
  @moduledoc """
  Explicit policy for images loaded from absolute filesystem paths.

  ## Fields and defaults

  | Field | Default |
  |---|---|
  | `enabled` | `false` |
  | `allowlist` | `[]` |
  | `follow_symlinks` | `false` |
  | `max_file_size` | `25_000_000` |
  | `extensions` | `[".png", ".jpg", ".jpeg", ".webp", ".gif", ".bmp", ".svg"]` |
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
