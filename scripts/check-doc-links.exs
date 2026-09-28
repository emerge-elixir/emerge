Code.require_file("../mix/release_checks.exs", __DIR__)

try do
  root =
    case System.argv() do
      [] -> "doc"
      [root] -> root
      _ -> raise ArgumentError, "usage: elixir scripts/check-doc-links.exs [doc]"
    end

  count = Emerge.Mix.ReleaseChecks.check_doc_links!(root)
  IO.puts("Checked local file links in #{count} documentation pages")
rescue
  error in [ArgumentError, File.Error] ->
    IO.puts(:stderr, Exception.message(error))
    System.halt(1)
end
