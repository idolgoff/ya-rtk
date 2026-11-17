# ya agent JSONL fixtures

`success.jsonl` is a captured successful `ya make` agent JSONL stream.
The only edits set the absolute epoch to `1` and replace the internal build
node path with `example/tests`. Event order, relative timestamps, build
progress, duration and final counts are from the capture.
`success.summary.txt` records the expected rendering for that fixture.

`mixed-test.jsonl` and `exception-epilogue.jsonl` are synthetic. Their event
shapes come from `ya --output-style=jsonl` build/test captures. The mixed
test combines observed event types to cover test counters, informational
stderr, explicit listings, slow tests, resource progress and final guidance.
The exception fixture models an outer traceback after a valid `finished` event.
