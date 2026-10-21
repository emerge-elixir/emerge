Code.require_file("../lib/emerge_skia/build_retry.ex", __DIR__)

{opts, rest, invalid} =
  OptionParser.parse_head(System.argv(), strict: [attempts: :integer, delay: :integer])

case {invalid, rest} do
  {[], [command | args]} ->
    {_, status} =
      EmergeSkia.BuildRetry.cmd(
        command,
        args,
        [stderr_to_stdout: true, into: IO.stream(:stdio, :line)],
        attempts: Keyword.get(opts, :attempts, 3),
        delay_ms: Keyword.get(opts, :delay, 10) * 1000
      )

    System.halt(status)

  _ ->
    raise ArgumentError, "usage: retry-build.exs [--attempts 3] [--delay 10] COMMAND [ARGS...]"
end
