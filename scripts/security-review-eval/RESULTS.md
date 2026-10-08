# SUB-1428 pilot assessment — 2026-10-08

The map did not improve defect detection on this small corpus: both arms found
all eight seeded defects in all three repetitions and raised no findings on the
eight safe counterparts. It did improve one specific source-evidence claim: all
three source-only reviews of the transitive-helper fixture incorrectly described
an internal module export as a public package export; map-assisted reviews did
not. That is a measured observation on one fixture, not a general improvement
claim. The map added about 5% to reported input tokens.

## Configuration and corpus

The [frozen protocol](README.md) used sixteen fixture packages, three repetitions
per arm, 96 sequential isolated reviews, shuffled with seed 1428. Codex CLI
0.160.1 requested `gpt-6-astra` with high reasoning effort and a 600-second
monotonic timeout. The compiler/runtime version was Submilli 0.3.0, based on
`f47e937a` plus the SUB-1428 working changes. The model identifier does not pin
provider weights. No trial was retried or excluded.

The four families each contain two unsafe/safe pairs: direct/transitive missing
authorization, swallowed denial/fallback authorization, identifier/path selector
mismatch, and cached disclosure/filesystem delivery. These are small synthetic
packages with explicit contracts, not maintained-package or remote-provider
reviews. Restricted-policy and authorized-use probes verified all sixteen
fixtures locally before the pilot. The delivery case tests local egress, not a
live network request.

Captured source and instructions matched across both arms and every repetition
for each fixture. The map arm additionally received the complete bounded
compiler-evidence bundle, including generated schemas. Both arms already had
identical checked-in package metadata. All 96 reports were complete; all 96 had
provider-reported token usage. There were no execution failures, unresolved
coverage questions, duplicate findings or unseeded defects in adjudication.

## Observations

| Measurement | Source only | Source plus map |
| --- | ---: | ---: |
| Completed reviews | 48/48 | 48/48 |
| Detected seeded defects | 24/24 | 24/24 |
| Missed seeded defects | 0 | 0 |
| Safe reviews without findings | 24/24 | 24/24 |
| False-alarm findings | 0 | 0 |
| Fully supported evidence, quality 2 | 21/24 | 24/24 |
| Evidence with an incorrect claim, quality 0 | 3/24 | 0/24 |
| Input tokens | 666,251 | 699,590 |
| Cached input tokens (included above) | 196,608 | 203,392 |
| Output tokens | 9,489 | 9,362 |
| Recorded reviewer time, total seconds | 782.415 | 826.525 |
| Recorded reviewer time, median seconds | 11.152 | 10.787 |

Each family had 6/6 detections and 6/6 quiet safe reviews in each arm. Every
unsafe fixture was detected in all three repetitions; every safe fixture stayed
quiet in all three. All 24 paired unsafe trials agreed on detection, and all 24
paired safe trials agreed on absence of findings. The evidence error occurred
in all three source-only repetitions of `c03`; no other evidence-quality
variation was observed under the rubric. There were no quality-1 findings.

The input-token increase was 33,339 (5.004%). Cache behavior differed between
arms, so these counts are not a dollar-cost estimate. Preparing the sixteen
captured compiler snapshots took 0.489 seconds in total; preparation was shared
by repetitions and arms, not charged six times per fixture.

### Why three evidence grades are zero despite correct detection

The assessor graded the 48 findings with explicit arm labels hidden, then
unblinded the saved judgments. Three reports correctly traced `save` to the
unchecked `persist` write but additionally claimed callers could invoke
`persist` directly as a package export. The package entry point exports only
`save`. A focused compile/run probe importing `persist` from `@acme/records`
failed with “does not export `persist`” and listed `save` as the available export.

Those findings remain true detections: the public `save` route demonstrably
reaches the unchecked write. The incorrect additional route claim receives
quality 0 under the frozen incorrect/unsupported-evidence rubric. It is not
counted as a separate false-alarm finding. Thus “zero false-alarm findings” must
not be read as “every factual claim was correct.” Blinded IDs were `b019`,
`b073`, and `b094`; after unblinding, these were `c03` source-only repetitions
1, 2, and 3.

## Limits and next steps

- Detection saturated on eight deliberately small unsafe examples. Three repeats
  of the same examples are not 24 independent defect classes. This pilot does
  not establish general recall, precision, or superiority on real packages.
- One assessor applied the rubric. Explicit arm labels were hidden during
  grading, but model prose can reveal use of compiler evidence. The actor also
  necessarily knows whether a map is present; this was not a double-blind study.
- The Mac repeatedly slept during the run. Monotonic timers excluded suspension,
  and some reviews were delayed across sleep/wake transitions. Temporary sleep
  assertions were added during the run and ended with its process. The recorded
  times above are descriptive only; they do not support a latency or throughput
  comparison. Early build/graph maintenance also overlapped the run.
- Keep compiler observations distinct from confirmed defects. The observed
  export-surface error is a useful target for a harder, held-out evaluation
  involving reexports, multiple modules and package boundaries. Do not tune this
  corpus and then claim improvement on it.
- No warning rule or CI security gate is promoted here. A future rule, including
  SUB-1496, still needs its own precise contract, abstention boundaries, safe and
  unsafe regressions, and full maintained-package finding audit under the gate
  in `plans/sub-1427-compiler-assisted-review.md`.

## Evidence retention

Raw prompts, responses, reports, usage, frozen corpus hashes, schedule and manual
judgments remain outside Git in `/tmp/sub1428-pilot-1` and
`/tmp/sub1428-adjudication-1`. The two-call pre-pilot smoke is separate at
`/tmp/sub1428-smoke-1` and is not included in these counts. Copy these external
artifacts when transferring the evaluation; temporary paths are not durable
storage. This curated assessment is the repository record, not a raw test dump.

The pilot’s portable corpus manifest SHA-256 (sorted compact JSON of relative paths to
file digests, excluding package-root `.submilli` editor caches) is
`f04032e9d99d82e79b84de2c8a038e0c90a6849aeaf12ab59d86e6acc4419a72`.
The pilot's original manifest also fingerprinted generated editor stubs and had
SHA-256 `f923e8ef7ea5e64a2c3ef79e5a9281098203798d1573d0aa3c0dded5da5b3b73`.
Those stubs were never reviewer inputs. The original raw manifest is preserved;
the scorer excludes its cache entries so transferring results does not require
untracked build files. The measured summary was reverified after removing those
caches. Future runs write the portable format directly.
Before publication, trailing spaces were removed from eight policy YAML files;
reviewer inputs and policy semantics are unchanged. The committed corpus manifest
SHA-256 is `6436c3312ef2b27f4cf5389b7ae27c3f738c01b9f53821caafedcfb5975adda0`.
The shared instruction skill SHA-256 is
`fb877ab9b8cc995e94d37bcc589b20bf985f43348f219387fef19c2d7d9b497e`.
Per-fixture source/map hashes and per-trial prompt hashes are in the external
records. Both arms' source and instruction equality was checked after the run.
