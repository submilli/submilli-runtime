# Paired package-security review pilot

SUB-1428 compares the existing security reviewer with and without compiler
information. The unit-test-only evaluation entry point reuses the production
snapshot, adapter, isolation, instructions, schema and response validation. It
adds no production CLI flags. Mock agents verify plumbing, not review quality.

## Frozen protocol

The corpus has eight minimal unsafe/safe pairs: two each for missing
caller authorization, swallowed denial, selector mismatch, and state/egress.
The latter covers a private cache and filesystem delivery of sensitive data;
it does not test an external network service. The ground truth in `cases.json`
and restricted policy probes in `policy/` are outside the reviewed package
roots. Fixture identifiers carry no safety label. The model sees only source,
package metadata/documentation, and optionally compiler evidence.

Run all sixteen cases three times in each arm (96 reviews), sequentially, using
Codex `gpt-6-astra` with high reasoning effort and a 600-second per-review timeout.
The runner shuffles the schedule with seed 1428 and records the exact schedule.
Each review uses a fresh ephemeral session with tools and ambient instructions
disabled by the existing adapter. There are no automatic retries. A smoke run is
separate from the pilot; do not tune fixtures or instructions after seeing pilot
results. Record any unavoidable protocol deviation explicitly.

Both arms use the same captured bytes, compiled once per fixture per run, and
the same instruction text and response schema. The source-only arm sets
`authority` to null: generated capability schemas, call graph, source-located
compiler facts, and map limitations are withheld together. This measures the
whole compiler-evidence bundle, not the graph alone. Checked-in source and
metadata remain identical. Preparation fails before any model call if any
fixture cannot compile or exceeds evidence limits.

The runner records source, map, skill and full-prompt hashes; compiler and agent
versions; requested model/effort; corpus fingerprints; monotonic preparation and
review durations; raw agent output; and provider-reported input, cached-input,
and output tokens. Input tokens include cached tokens; do not add cached input
a second time. Missing or malformed usage remains unavailable, never zero or an
estimate. The requested model identifier is not proof of an immutable provider
model revision. Provider caching and service latency remain confounders, so
report cached usage separately and do not interpret latency as native CPU time.
Corpus manifest format 2 excludes package-root `.submilli` editor caches, which
snapshot collection never supplies to the reviewer. The scorer also ignores
those cache entries in the first pilot's legacy manifest; it still verifies all
actual fixture, policy and ground-truth files. Policy verification works on
temporary package copies and leaves the corpus unchanged.

## Run

From the repository root, build and verify the affected implementation:

```sh
SUBMILLI_SKIP_HTTP_TESTS=1 SUBMILLI_FULL_TEST=0 cargo test -p submilli --bin submilli security_review
SUBMILLI_SKIP_HTTP_TESTS=1 SUBMILLI_FULL_TEST=0 cargo test -p submilli --test security_review
python3 scripts/security-review-eval/verify_policy.py --binary target/debug/submilli
python3 -m unittest discover -s scripts/security-review-eval -v
```

The policy verifier publishes each package into its own temporary store. It
runs the attack under restricted caller permissions, verifies the denied or
observable effect, then confirms authorized use still works. The fallback pair
also verifies that independent fallback authority suffices when primary access
is denied. It never calls a model or external service. These are Submilli policy
fixtures using host APIs and typed catches, not a TypeScript/Node parity claim.

`test_process.py` is opt-in: set `REVIEW_EVAL_TEST_BINARY` to the unit-test
executable reported by Cargo to run its mock process checks. The production
process tests above also cover timeout cleanup and all supported adapters.

Prepare without calling a model, then run a live smoke and inspect its reports
and metrics before starting the pilot. Output paths must not exist. The runner
builds the unit-test executable automatically unless `--test-binary` is given.
It uses only the exact ignored evaluation test, never a full test suite.

```sh
python3 scripts/security-review-eval/run.py --prepare-only --output /tmp/review-prepare
python3 scripts/security-review-eval/run.py --case c02 --repeats 1 --output /tmp/review-smoke
python3 scripts/security-review-eval/run.py --output /tmp/review-pilot
```

Live runs require an installed, authenticated Codex CLI and network access to
its model service. Fixture execution itself remains local. Verify both smoke
reports are complete and token usage is present. Do not start 96 reviews if the
smoke exposes authentication, model, adapter or telemetry failures.

## Adjudicate before unblinding

```sh
python3 scripts/security-review-eval/score.py blind /tmp/review-pilot /tmp/review-adjudication
# Read packets.json and complete judgments.json without reading mapping.json.
python3 scripts/security-review-eval/score.py aggregate /tmp/review-pilot /tmp/review-adjudication
```

The blind packet contains source and findings but omits explicit arm labels,
trial identities and map hashes. Model prose may still reveal map use; this is
label blinding, not a guarantee that an assessor cannot infer an arm. Ground
truth is available to the assessor, never to the actor. Assess the mechanism and
source evidence, not wording or severity. Classify every finding:

- `seed`: correctly identifies the seeded authority violation. Count once.
- `duplicate`: repeats another adjudicated finding in that report.
- `false_alarm`: claims a defect that the source and stated contract do not support.
- `other_defect`: identifies a real unseeded defect. This invalidates the clean
  corpus assumption; investigate and report it rather than calling it a false alarm.

Assign evidence quality 0 (incorrect/unsupported), 1 (correct but incomplete), or
2 (correct source reference, authority path and consequence, actionable fix),
and write a rationale. Automated scoring validates coverage of judgments and
report hashes; it cannot prove the assessor's judgment correct.

Report detections, complete-review misses, false alarms, duplicates, evidence
quality, incomplete reviews, failed/not-run trials, token availability and
cost, and elapsed time by arm and family. Detections from incomplete reviews
remain visible, but an incomplete or failed empty report is not a clean review
or a complete-review miss. Paired results require matching source hashes and
complete reviews in both arms. Inspect per-repeat outcomes as well as aggregates.

This is a small synthetic pilot, not a maintained-package audit, statistical
proof of improvement, or security certificate. Its small, mostly straight-line
fixtures may be easy for source-only review. Neutral or negative results are
valid. Commit only the curated assessment with limitations and next steps;
keep prompts, transcripts, raw metrics and adjudication files outside Git.
