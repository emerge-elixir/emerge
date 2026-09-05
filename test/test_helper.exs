Code.require_file("support/animated_hit_case_helper.exs", __DIR__)

excluded_tags = [full_sweep: true, headless_prime_hardware: true]

excluded_tags =
  if :os.type() == {:unix, :linux}, do: excluded_tags, else: [:linux_only | excluded_tags]

ExUnit.start(exclude: excluded_tags)
