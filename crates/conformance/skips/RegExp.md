# RegExp

Source: `test/built-ins/RegExp/**` (~1900 files) plus the regex-arm
`String/prototype/{split,replaceAll}` vectors (the other regex-backed String
methods were ported with the String area). The engine is the Rust `regex`
crate behind `submilli:regex` (docs/regex.md), so this area carries the
engine-divergence pins alongside the conformance port: 47 cases under
`cases/RegExp/` (2 `expect-error` divergence pins, 9 `expect-fail` known
gaps) + 7 regex-arm cases under `cases/String/prototype/{split,replaceAll}/`,
all passing. Representative rejected originals under `rejected/RegExp/`.

Blanket rules (SKIPS.md) cover `prop-desc.js`, `length.js`, `name.js`,
`not-a-constructor.js`, `is-a-constructor.js`, `proto-from-ctor-realm.js`,
`cross-realm.js`, every `Symbol.*` protocol directory
(`prototype/Symbol.{match,matchAll,replace,search,split,species}/**` — String
methods take a `RegExp` directly; the protocol is not user-visible), the
`coercion-*` / `*-coerce*` / `this-val-*` receiver- and argument-coercion
matrices, and the `eval`-based cases (`S15.10.1_A1_T*`, `escape/**`,
`source/value-*` which rebuild regexes via `eval`).

Porting adaptations used throughout (README rules):

- `RegExpExecArray` (array with `.index`/`.input`/`.groups` properties)
  becomes the `RegExpMatch` interface: `m[0]` → `m.match`, `m[i]` →
  `m.groups[i-1]` (`(string | undefined)[]`, unmatched = `undefined`), `m.length` →
  `m.groups.length + 1`, `result.groups.name` → `m.namedGroups.get("name")`.
- Narrowing is invalidated by any call, so ported cases copy every
  `RegExpMatch` field into locals immediately after the null check before the
  first shim assertion.
- Control characters and non-ASCII code points in test strings are spelled
  with `\uHHHH` escapes; lone-surrogate rows are dropped (the engine matches
  over UTF-8, where an unpaired surrogate reads as U+FFFD; match results keep
  the input's code units).
- `new String(...)` receivers become plain strings; `__split.constructor`
  checks are dropped.

## Divergence pins (`cases/RegExp/divergence/`)

Documented engine divergences, pinned to the *Submilli* behavior (an original
asserting the JS behavior would be a permanent failure by design):

| Pin | Divergence |
|:--|:--|
| `lastindex-readonly` (`expect-error`) | `lastIndex` is read-only in v1 (RegExp builtin declaration documents it); JS honors user writes. Every test262 case that sets `lastIndex` (`exec/y-*-lastindex*`, `test/y-*`, `S15.10.6.2_A4_T*`, `A5_T2/T3`) is unportable until the writable follow-up lands; this pin flips when it does. |
| `pattern-syntax-error-at-compile-time` (`expect-error`) | Regex literals are validated at codegen time (docs/regex.md), so `/0{2,1}/` is a compile diagnostic, not a catchable runtime SyntaxError (from `15.10.2.5-3-1.js`). |
| `dot-astral-without-u` | `.` always matches a whole code point; JS without `u` works on code units, so a single `.` never matches an astral character. The same holds for empty matches in `replace` and `split`: without `u`, JS steps one code unit past an empty match, splitting an astral character into its halves, while Submilli steps a whole code point. (docs/regex.md describes the no-u fallback as byte-mode; observed behavior is code-point-mode — the doc's description of the mechanism is stale, the divergence-from-JS is real either way.) |
| `replaceall-nonglobal-no-typeerror` | `replaceAll` with a non-g regex replaces every match instead of throwing TypeError (prelude doc-comment documents the divergence; from `replaceAll/searchValue-flags-no-g-throws.js`). |

## Known gaps (`expect-fail`)

| Case | Gap |
|:--|:--|
| `S15.10.2.13_A1_T1`, `S15.10.2.13_A2_T2` | `[]` / `[^]` are valid ECMA-262 classes (match nothing / match anything); the engine rejects both at compile time ("unclosed character class"). A translator rewrite (`[^\s\S]` / `[\s\S]`) would close this. |
| `S15.10.2.10_A2.1_T1` | Control escapes `\cA`..`\cZ` are rejected ("unrecognized escape sequence"). |
| `S15.10.2.10_A5.1_T1` | `\<` and `\>` should be identity escapes; the engine parses them as start/end word-boundary assertions, so they never match the literal character. All other punctuation identity escapes pass (probed). |
| `S15.10.2.5_A1_T4` | Captures inside a quantified group are not cleared on iterations where they don't participate (ECMA RepeatMatcher zeroes them): capture 4 of `/(z)((a+)?(b+)?(c))*/` on `"zaacbbbcac"` is `"bbb"`, JS says undefined. |
| `nullable-quantifier` | `/(a?b??)*/` on `"ab"` matches only `"a"`; ECMA's RepeatMatcher empty-iteration rule lets JS match `"ab"`. |
| `dotall/with-dotall`, `dotall/without-dotall` | `.` excludes only LF, not CR/U+2028/U+2029, and matches whole astral code points (see the divergence pins; the LineTerminator exclusions are an unpinned, undocumented gap). |
| `named-groups/non-unicode-match` | `(?<$>...)` — `$` is a valid ECMA GroupName character; the engine rejects it ("invalid capture group character"). The non-`$` rows are covered by the passing `unicode-match` port. |

## Rejected (design decisions)

| Pattern | Reason |
|:--|:--|
| Lookahead (`S15.10.2.8_A1_T*`, `lookahead-quantifier-match-groups.js`, `unicode_restricted_quantifiable_assertion.js`), lookbehind (`lookBehind/**`, `named-groups/lookbehind.js`), backreferences (`S15.10.2.8_A1_T4`+, `S15.10.2.9_A*`, `S15.10.2.6_A3_T7`+, `multiline-match-done.js`, the `\k<name>` rows of `named-groups/*-references.js` and `duplicate-names-*.js`) | Linear-time engine, by design (docs/regex.md). Compile errors already pinned by the interpreter fixtures `expect_error_regex_{lookahead,negative_lookahead,lookbehind,negative_lookbehind,backreference,named_backref}.subm`. Copied: `rejected/RegExp/S15.10.2.8_A1_T1.js`, `rejected/RegExp/S15.10.2.9_A1_T2.js`, `rejected/RegExp/lookBehind/simple-fixed-length.js`. |
| `prototype/Symbol.{match,matchAll,replace,search,split}/**`, `Symbol.species/**`, `call_with_*` / `from-regexp-like*` (Symbol.match-driven IsRegExp) | Symbol protocol is not user-visible. Copied: `rejected/RegExp/prototype/Symbol.replace/exec-invocation.js`. |
| `match-indices/**`, the `d`-flag rows of `duplicate-flags.js`, `prototype/hasIndices/**` | `d` flag (hasIndices) deferred (docs/regex.md). Copied: `rejected/RegExp/match-indices/indices-array.js`. |
| `unicodeSets/**`, `prototype/unicodeSets/**`, `regexp-v-flag` material | `v` flag (ES2024) deferred pending regex-crate support (docs/ecma-262-gaps.md §22). Copied: `rejected/RegExp/unicodeSets/generated/character-class-difference-character.js`. |
| `annexB/built-ins/RegExp/**` (legacy accessors `RegExp.$1`/`lastMatch`/…, `prototype/compile`, web-reality octal/decimal escapes, `\cX` class fallbacks) | Annex-B blanket rule. Copied: `rejected/RegExp/annexB/legacy-accessors/lastMatch/this-not-regexp-constructor.js`, `rejected/RegExp/annexB/prototype/compile/B.RegExp.prototype.compile.js`, `rejected/RegExp/annexB/RegExp-decimal-escape-not-capturing.js`. |
| `prototype/toString/**`, `S15.10.6.4_A*` | No `RegExp.prototype.toString` method; `String(re)` returns `/source/flags` via the `$Object` vtable and is covered by the interpreter fixture `regex_to_string.subm`. |
| `regexp-modifiers/**`, `early-err-modifiers-*`, `syntax-err-arithmetic-modifiers-*` | Inline-modifier proposal (`(?i:...)`) — not in the supported subset. |
| `property-escapes/**`, `named-groups/*-property-names*.js` | `\p{...}` property escapes — not in the supported subset (untested against the engine; revisit if exposed). |
| `S15.10.3.1_A*` (RegExp(...) as function call), `S15.10.4.1_A*` (constructor-argument coercion, `new RegExp(regexp)`), `S15.10.5_A*`, `prototype/15.10.6.js`, `no-regexp-matcher.js` | Function-call form, RegExp-from-RegExp cloning, and prototype-object identity — `new RegExp(source: string, flags: string)` is the only constructor surface. |
| `15.10.2.15-6-1.js`, `15.10.2.5-3-1.js` (as runtime throws), `duplicate-named-capturing-groups-syntax.js` | Pattern early errors are compile-time diagnostics for literals (pinned by `divergence/pattern-syntax-error-at-compile-time.ts`); dynamic `new RegExp` failures throw a catchable `Error` (probed — the docs/regex.md "uncatchable trap" note is stale). Duplicate named groups (ES2025) are additionally rejected by the regex crate itself even in disjoint alternatives. |

## Not ported (below the curation bar)

| Pattern | Reason |
|:--|:--|
| Remaining `S15.10.2.3_A1_T*`, `S15.10.2.6_A*`, `S15.10.2.7_A*`, `S15.10.2.8_A2/A3/A4_T*`, `S15.10.2.13_A*` rows | Same constructs as the ported representatives (alternation, anchors, `\b`/`\B`, quantifiers, classes, dot) over different inputs; the interpreter fixtures `regex_*.subm` and `conformance_regexp.subm` add more. The rows built on backreferences or `[^]`/`[\b]` fall under the rejected/gap entries above. |
| `S15.10.2.10_A1.2-A1.5`, `S15.10.2.11_A1_T*` (`\0`, DecimalEscape), `S15.10.2.12_A*` and `CharacterClassEscapes/**` | Escape-table repetition; `\t`/`\x`/`\u` representatives ported; `\s`/`\d`/`\w` semantics are covered by the interpreter fixture `regexp_shorthand_classes_match_js.ts`. The 0x10000-code-point sweeps (`character-class-escape-non-whitespace.js`) are too heavy for the harness. |
| `prototype/exec/S15.10.6.2_A1_T*` remainder, `prototype/test/S15.10.6.3_A1_T*` remainder | Receiver/argument coercion variants (`new String`, `new Object`, functions, `eval`) of the ported T1/T2 intent. |
| `prototype/exec/u-lastindex-*`, `y-*`, `failure-*`, `success-*`, `prototype/test/y-*` | All require writing `lastIndex` or property-descriptor traps — blocked on the read-only divergence pin (`divergence/lastindex-readonly`). |
| `prototype/{global,ignoreCase,multiline,dotAll,sticky,unicode,source,flags}/**` | Accessor prop-desc/this-coercion matrices; the flag-property reads themselves are covered by `valid-flags-y.ts` and the interpreter fixtures `regex_property_reads.subm` / `regex_flags_all.subm`. |
| `named-groups/duplicate-names-*.js`, `groups-object*.js`, `string-replace-get.js`, `functional-replace-*` | Duplicate names are engine-rejected (recorded above); groups-object shape and `Array.prototype.groups` pollution don't exist under `namedGroups`; function replacers are deferred by design (docs/regex.md); the expressible `$<name>`/`$N` token semantics are covered by the ported `string-replace-*` cases. |
| `quantifier-integer-limit.js`, `regexp-class-chars.js`, `u180e.js`, `character-class-escape-non-whitespace-u180e.js` | Marginal single-row vectors (integer-limit quantifiers, U+180E membership churn across Unicode versions). |
| `String/prototype/split/**` remaining regex-separator rows | Same split semantics as the five ported rows; the coercion/`undefined`-separator variants are type-rejected (recorded with the String area). |
