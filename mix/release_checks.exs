defmodule Emerge.Mix.ReleaseChecks do
  @moduledoc false

  def release_notes!(tag, changelog, mix_version, cargo_version) do
    if tag != "v#{mix_version}" or mix_version != cargo_version do
      raise ArgumentError, "Release tag and Mix/Cargo versions must match"
    end

    heading = ~r/^## \[#{Regex.escape(mix_version)}\] - (\d{4}-\d{2}-\d{2})$/m

    {header, date} =
      case Regex.scan(heading, changelog) do
        [[header, date]] -> {header, date}
        _ -> raise ArgumentError, "Expected one dated changelog section for this release"
      end

    Date.from_iso8601!(date)
    [_, remainder] = String.split(changelog, header, parts: 2)
    notes = remainder |> String.split(~r/^## /m, parts: 2) |> hd() |> String.trim()

    if notes == "" or String.contains?(notes, "Draft release notes") do
      raise ArgumentError,
            "Finalize the changelog date and remove the draft notice before tagging"
    end

    notes
  end

  def release_notes_from_files!(tag, root \\ ".") do
    # Read this repository's explicit version literals without evaluating build code.
    mix_version = capture!(~r/^\s*@version "([^"]+)"/m, File.read!(Path.join(root, "mix.exs")))
    cargo = File.read!(Path.join(root, "native/emerge_skia/Cargo.toml"))
    package = capture!(~r/^\[package\]\s*\n(.*?)(?=^\[|\z)/ms, cargo)
    cargo_version = capture!(~r/^version\s*=\s*"([^"]+)"/m, package)
    release_notes!(tag, File.read!(Path.join(root, "CHANGELOG.md")), mix_version, cargo_version)
  end

  def check_doc_links!(root) do
    pages = root |> Path.join("**/*.html") |> Path.wildcard() |> Enum.sort()
    if pages == [], do: raise(ArgumentError, "No HTML documentation found in #{root}")
    missing = Enum.flat_map(pages, &missing_links/1)

    if missing != [] do
      raise ArgumentError, "Missing documentation links:\n" <> Enum.join(missing, "\n")
    end

    length(pages)
  end

  def missing_links(page) do
    # ExDoc emits ordinary HTML tags. Ignore comments and raw script/style bodies,
    # preserving their opening tags so external script sources are still checked.
    html =
      page
      |> File.read!()
      |> then(&Regex.replace(~r/<!--.*?-->/s, &1, ""))
      |> then(
        &Regex.replace(
          ~r/(<(?:script|style)\b(?:[^>"']|"[^"]*"|'[^']*')*>).*?(<\/(?:script|style)\s*>)/is,
          &1,
          "\\1\\2"
        )
      )

    ~r/<[a-z][a-z0-9:-]*(?:[^>"']|"[^"]*"|'[^']*')*>/i
    |> Regex.scan(html)
    |> Enum.flat_map(fn [tag] ->
      ~r/\s([a-z_:][a-z0-9_.:-]*)\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))/i
      |> Regex.scan(tag, capture: :all_but_first)
      |> Enum.filter(fn [name | _] -> String.downcase(name) in ["href", "src"] end)
      |> Enum.map(fn [_name | values] -> Enum.find(values, &(&1 != "")) end)
    end)
    |> Enum.reject(&is_nil/1)
    |> Enum.filter(&missing_reference?(&1, page))
    |> Enum.map(&"#{page}: #{&1}")
  end

  defp missing_reference?(reference, page) do
    uri = reference |> String.replace("&amp;", "&") |> URI.parse()
    path = uri.path

    is_nil(uri.scheme) and is_nil(uri.host) and is_binary(path) and path != "" and
      not String.starts_with?(path, "/") and path != "docs_config.js" and
      not File.exists?(Path.join(Path.dirname(page), URI.decode(path)))
  end

  defp capture!(pattern, text) do
    case Regex.run(pattern, text, capture: :all_but_first) do
      [value] -> value
      _ -> raise ArgumentError, "Missing explicit Mix/Cargo package version"
    end
  end
end
