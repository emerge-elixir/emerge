# CI download retries: remaining validation

Local quality, full Elixir/Rust tests and workflow lint pass. The supplied Skia
HTTP 503 failure is covered by fixtures, including bounded retries and cache-safe
command reuse; compiler, test and integrity failures remain terminal.

- Run the updated Linux/macOS CI and grouped release builders remotely to verify
  the standalone wrapper, streamed logs and preserved build caches on clean runners.
- Retire this plan after remote acceptance; implementation details live in
  `EmergeSkia.BuildRetry` and the native-build internal guide.
