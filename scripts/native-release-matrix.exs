Code.require_file("../lib/emerge_skia/build_profiles.ex", __DIR__)

jobs =
  case System.argv() do
    [] -> EmergeSkia.BuildProfiles.release_jobs()
    ["--hosts"] -> EmergeSkia.BuildProfiles.host_release_jobs()
    _ -> raise ArgumentError, "usage: native-release-matrix.exs [--hosts]"
  end

IO.puts(JSON.encode!(jobs))
