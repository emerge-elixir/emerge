Code.require_file("../mix/release_checks.exs", __DIR__)

try do
  case System.argv() do
    [tag] -> IO.puts(Emerge.Mix.ReleaseChecks.release_notes_from_files!(tag))
    _ -> raise ArgumentError, "usage: elixir scripts/release-notes.exs vVERSION"
  end
rescue
  error in [ArgumentError, File.Error] ->
    IO.puts(:stderr, Exception.message(error))
    System.halt(1)
end
