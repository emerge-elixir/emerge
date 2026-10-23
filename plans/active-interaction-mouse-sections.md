# Mouse examples under Interaction

- [x] Compose Mouse coordinates and Mouse leave as named Interaction sections;
  remove their standalone navigation entries and page routing.
- [x] Keep existing controllers, captured drag behavior and 1x/1.6x/2x demos.
- [x] Update example grouping, navigation tests and README; verify both demos
  remain rendered and routed under Interaction.
- [x] Run demo formatting, warnings-as-errors compile, mix test and cargo test.

Only demo presentation changes are needed; preserve all prior work and deps.

## Validation

Both named sections are composed after mouse down/up in Interaction. Their
controllers and scale/drag behavior are unchanged. Example keys now belong to
Interaction (15 examples total); the two standalone tabs/routes are removed.
The composed routes, navigation, native input and small-viewport regressions pass.

- Demo formatting and warnings-as-errors compile: passed.
- Demo mix test: 121 passed (two standalone-page generated cases removed).
- Cargo test: 1,637 unit plus 14 integration tests, all passed.
- No native source changes; no native-library restart needed for this move.
- Logs: /tmp/emerge-interaction-mouse-demo.log and
  /tmp/emerge-interaction-mouse-rust.log.
