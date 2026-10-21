defmodule EmergeSkia.BuildRetry.Output do
  @moduledoc false
  defstruct [:sink]
end

defimpl Collectable, for: EmergeSkia.BuildRetry.Output do
  # Stream complete logs while retaining only a bounded tail for classification.
  def into(%{sink: sink}) do
    {initial, collect} = Collectable.into(sink)

    {{initial, ""},
     fn
       {acc, tail}, {:cont, bytes} ->
         tail = tail <> bytes
         offset = max(0, byte_size(tail) - 131_072)
         {collect.(acc, {:cont, bytes}), binary_part(tail, offset, byte_size(tail) - offset)}

       {acc, tail}, :done ->
         collect.(acc, :done)
         tail

       {acc, _tail}, :halt ->
         collect.(acc, :halt)
     end}
  end
end

defmodule EmergeSkia.BuildRetry do
  @moduledoc false

  @network_errors [
    ~r/(?:requested URL returned error:|HTTP(?:\/[\d.]+)?(?:\s+(?:status|error))?(?:\s+code)?[:\s]+|status code[:\s]+)\s*(?:408|429|5\d\d)\b/i,
    ~r/failed to get successful HTTP response[^\n]*\bgot (?:408|429|5\d\d)\b/i,
    ~r/\bcurl(?::\s*\(|\s+)(?:5|6|7|18|28|35|52|55|56|92)(?:\)|\s)/i,
    ~r/\[(?:5|6|7|18|28|35|52|55|56|92)\]\s*(?:Could not|Couldn't|Timeout|Failed|Failure|SSL|Stream|Transferred|Operation)/i,
    ~r/(?:fatal: unable to access|failed to download|failed to fetch)[^\n]*(?:connection (?:reset|refused|timed out)|could not resolve|temporary failure in name resolution|unexpected eof)/i
  ]
  @hard_errors [
    ~r/\berror\[E\d+\]|^\s*error[^\n]*\n\s*-->/m,
    ~r/\.(?:c|cc|cpp|cxx|h|hpp):\d+(?::\d+)?:\s*(?:fatal )?error:/i,
    ~r/checksum[^\n]*(?:mismatch|fail|invalid|does not match)|hash (?:mismatch|sum mismatch)|signature[^\n]*(?:invalid|fail)/i,
    ~r/test result: FAILED|\bFailed: \d+ tests?\b|Compilation failed due to warnings|error occurred in cc-rs/i,
    ~r/\*\* \((?:CompileError|SyntaxError|TokenMissingError|MismatchedDelimiterError|UndefinedFunctionError)\)/
  ]

  def transient?(output) do
    output = Regex.replace(~r/\e\[[0-?]*[ -\/]*[@-~]/, output, "")

    Enum.any?(@network_errors, &Regex.match?(&1, output)) and
      not Enum.any?(@hard_errors, &Regex.match?(&1, output))
  end

  def cmd(program, args, cmd_opts \\ [], opts \\ []) do
    attempts = Keyword.get(opts, :attempts, 3)
    delay = Keyword.get(opts, :delay_ms, 10_000)

    unless is_integer(attempts) and attempts in 1..10 and is_integer(delay) and delay >= 0,
      do: raise(ArgumentError, "attempts must be 1..10 and delay must be non-negative")

    config = %{
      attempts: attempts,
      delay: delay,
      run: Keyword.get(opts, :run, &System.cmd/3),
      sleep: Keyword.get(opts, :sleep, &Process.sleep/1),
      report: Keyword.get(opts, :report, &IO.puts(:stderr, &1))
    }

    cmd_opts =
      if sink = cmd_opts[:into],
        do: Keyword.put(cmd_opts, :into, %EmergeSkia.BuildRetry.Output{sink: sink}),
        else: cmd_opts

    attempt(program, args, cmd_opts, config, 1)
  end

  defp attempt(program, args, cmd_opts, config, number) do
    {output, status} = result = config.run.(program, args, cmd_opts)

    if status not in [0, 126, 127, 130, 137, 143] and number < config.attempts and
         transient?(output) do
      delay = config.delay * number

      config.report.(
        "Transient download failure; retrying #{program} in #{div(delay, 1000)}s " <>
          "(attempt #{number + 1}/#{config.attempts})."
      )

      config.sleep.(delay)
      attempt(program, args, cmd_opts, config, number + 1)
    else
      result
    end
  end
end
