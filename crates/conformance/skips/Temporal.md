# Temporal

Sources: `test/built-ins/Temporal/**` (4,603 files: Instant 465, Duration 540,
PlainDate 652, PlainTime 493, PlainDateTime 773, PlainYearMonth 509,
PlainMonthDay 199, ZonedDateTime 901, Now 66, plus namespace tests) and
`test/staging/Temporal/**` (2). 41 ported (33 passing, 8 `expect-fail`),
under `cases/Temporal/`, mirroring the test262 tree (the one staging port
lives at `cases/Temporal/staging/`). Representative rejected originals under
`rejected/Temporal/` (one sourced from `test/intl402/Temporal/`, noted in its
header, as the non-ISO-calendar representative — built-ins has no non-ISO
material).

Submilli narrows Temporal by design: ISO-8601 only, time zones are IANA-id or
fixed-offset strings, no `Temporal.Calendar`/`Temporal.TimeZone` objects, no locale
formatting, and `from()` takes ISO strings (not property bags). The blanket
rules (SKIPS.md) cover every per-member `prop-desc.js` / `length.js` /
`name.js` / `not-a-constructor.js` / `builtin.js` / `branding.js` /
`subclassing-ignored.js` / `toStringTag/` / `constructor.js` and the
coercion-trap (`order-of-operations.js`, `options-wrong-type.js`,
`*-wrong-type.js`, `roundingincrement-nan.js`, …) material without further
note. Standard adaptations used throughout the ports: positional `new
Temporal.<Type>(...)` constructors → `from(ISO string)` /
`fromEpochNanoseconds()` / `new Temporal.Duration({...})` (the field-bag
form), `TemporalHelpers.assert*` → field-by-field `assertSameValue` helpers,
heterogeneous tuple tables → typed helper functions, numeric separators
dropped from bigint literals.

## Rejected (design decisions)

| Pattern | Reason |
|:--|:--|
| Calendar machinery: `calendar-temporal-object.js`, `argument-propertybag-calendar-*.js`, `calendarId`, `withCalendar/**`, `era`/`eraYear`, `monthCode`-as-input cases, `calendarname-*.js` toString options | ISO-8601 only; no calendar objects, no calendar ids on the API surface. Copied: `rejected/Temporal/PlainDate/from/calendar-temporal-object.js`, `rejected/Temporal/ZonedDateTime/prototype/withCalendar/calendar-string.js`. |
| Non-ISO calendars (all of `test/intl402/Temporal/**`) | ISO-8601 only by design. Copied: `rejected/Temporal/PlainDateTime/from/era-japanese.js`. |
| `TimeZone` objects and timezone-like property bags (`argument-propertybag-timezone-wrong-type.js`, `timezone-getpossibleinstantsfor-*`, `getTimeZoneTransition/**`) | Time zones are IANA-id strings; no TimeZone objects. Copied: `rejected/Temporal/ZonedDateTime/from/argument-propertybag-timezone-wrong-type.js`. |
| Property-bag argument coercion (`order-of-operations.js`, `argument-invalid-property.js`, `argument-singular-properties.js`, "incorrectly-spelled properties are ignored" arms, getter traps like `argument-object-get-*-throws.js`, `options-read-before-algorithmic-validation.js`) | Bags are typed literals: misspelled/extra fields and observable getters are compile errors or inexpressible. Copied: `rejected/Temporal/Instant/prototype/add/order-of-operations.js`. Ports of mixed cases drop only those arms (noted in each file). |
| ToTemporal* coercion of wrong-typed arguments (`argument-number.js`, `argument-wrong-type.js`, `argument-not-object.js`, `argument-string-invalid.js` arms that pass non-strings) | Statically typed arguments make the coercing call a compile error. Copied: `rejected/Temporal/PlainDate/from/argument-number.js`. |
| `valueOf/**` and relational-operator cases | No boxing; `<`/`>` on Temporal values is a compile-time type error (spec.md). Copied: `rejected/Temporal/Instant/prototype/valueOf/basic.js`. |
| `toLocaleString/**` | No locale/ICU. Copied: `rejected/Temporal/Instant/prototype/toLocaleString/return-string.js`. |
| `Temporal/getOwnPropertyNames.js`, `keys.js`, `prop-desc.js`, `toStringTag/**`, `subclass.js`, `get-prototype-from-constructor-throws.js` | Namespace reflection, descriptors, subclassing — no prototypes/descriptors. |
| Leap-second strings (`*leap-second.js`), `year-zero.js` minus-zero strings, annotation-syntax torture (`argument-string-*-annotation*.js`, `argument-string-multiple-*.js`, critical flags) | ISO-string parser torture for syntax we deliberately keep minimal; reconsider with a parser-conformance push. |

## Known gaps (`expect-fail`)

Already tracked in the gaps doc (`docs/ecma-262-gaps.md` Temporal section):

| Case | Gap |
|:--|:--|
| `PlainTime/prototype/round/rounding-cross-midnight` | No `round()` on `PlainTime`. |
| `ZonedDateTime/prototype/withPlainTime/*` | `withPlainTime` is not declared or implemented. |

Newly discovered while porting (not in the gaps doc):

| Case | Gap |
|:--|:--|
| `Duration/prototype/abs/basic`, `Duration/prototype/negated/basic` | Duration slots are capped far below the standard's 2^32 limit: `{ years: 1e4 }` constructs, `{ years: 1e5 }` throws "overflows the maximum duration". |
| `Instant/limits.ts` | Representable Instant range is narrower than ±8.64e21 ns (±1e22−1e9 already rejected), and the six-digit extended-year forms (`-271821-04-20T00:00:00Z`) don't parse. |
| `PlainDate/from/limits` | Same extended-year parse hole caps the PlainDate range below ±271821/275760. |
| `PlainDate/from/argument-string` | `PlainDate.from` accepts only the extended 4-digit-year `YYYY-MM-DD` form: compact (`19761118`), `±YYYYYY` extended years, and datetime-with-offset strings all rejected. |
| `PlainTime/prototype/add/argument-object`, `PlainDateTime/prototype/add/hour-overflow` | `nanosecond` on PlainTime/PlainDateTime returns nanoseconds-within-*second* (e.g. 123456789) instead of the standard within-*microsecond* (0–999). `millisecond`/`microsecond` are standard, and `ZonedDateTime.nanosecond` is correct — only these two types diverge. |

Other gaps hit while probing, recorded here but not pinned by a port:
property-bag `from({ year, month, day })` is a compile error on every type
(`from` is string-only; the standard accepts field bags); no
`overflow: "constrain" | "reject"` option on `from`/`add`/`with`;
fixed-offset time-zone ids and bracketed offset annotations are supported.

## Not ported (curation)

| Pattern | Why |
|:--|:--|
| `Now/**` | Non-deterministic by nature; existing fixtures (`temporal_now_*.subm`) smoke-test the surface. |
| Per-rounding-mode matrices (`roundingmode-{ceil,floor,expand,halfEven,…}.js` × every type/method) and `roundingincrement-*` grids | Huge generated matrices; representative modes are covered by `Instant/until`, `Duration/round`, and the existing `temporal_until_options.subm` fixture. |
| `argument-string-*` parse-variant grids (separators, offsets, sub-minute offsets, multiple offsets) | Parser torture beyond the minimal ISO forms we guarantee; revisit alongside the extended-year gap. |
| `argument-duration-max.js`, `*-out-of-range.js`, `infinity-throws-rangeerror.js`, `non-integer-throws-rangeerror.js` per method | Limit/validation behavior is pinned once via `Instant/limits`, `PlainDate/from/limits`, and the Duration cap rows above; per-method repeats add no signal while the range gaps are open. |
| `equals`/`compare` argument-casting variants (`argument-zoneddatetime.js`, `instant-string.js`, `argument-cast.js`, exhaustive grids) | Arguments are statically typed — implicit casting from strings/bags doesn't exist; the typed-value comparisons are ported (`Instant/compare/cross-epoch`, `Instant/prototype/equals/basic`, `PlainTime/prototype/equals/basic`). |
| `PlainMonthDay/from/basic.js`, `fields-leap-day.js` and other bag-based `from` cases | Blocked behind the property-bag `from` gap above; the string forms are covered by `temporal_plainmonthday.subm` and `PlainMonthDay/prototype/toPlainDate/basic`. |
| `Duration/prototype/with/sign-conflict-throws-rangeerror.js` | Built on computed property keys (`{ [field]: -1 }`), not in the subset; the merge semantics are ported via `partial-positive`. |
| `toString` option grids (`fractionalseconddigits-*`, `smallestunit-*`, `calendarname-*`, `timezonename-*`) | `toString(options)` exists only on Duration here (ported: `toString/precision`); the other types' options are part of the tracked round/format cluster. |
| `PlainDateTime/prototype/toZonedDateTime/basic.js` and disambiguation variants | Pinned through `epochNanoseconds`/`calendarId` (both unavailable); conversion itself is covered by `PlainDate/prototype/toZonedDateTime/basic` and `temporal_plain_to_zoned.subm`. |
