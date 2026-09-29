# Error

Source: `test/built-ins/Error/**` (93 files). spec.md §1.8 defines `Error`
as a class with mutable `message` and `name` fields, a required `message`
constructor argument, `toString()`, `Error.isError`, and user subclasses via
`extends`. Most of the area tests prototype objects, property descriptors,
the `stack` accessor, and realms, which don't exist here.

Ported: 13 (11 passing, 2 `expect-error` pins). Representative rejected
originals under `rejected/Error/`.

Where the original calls `new Error()`, the port uses `new Error("")`: the
constructor requires a message, and an empty message reaches the same
`toString` step as an absent one.

| Pattern | Disposition | Reason |
|:--------|:------------|:-------|
| `message_property.js` | ported, passing | Value check and write kept; descriptor half dropped. |
| `prototype/toString/15.11.4.4-{6-1,6-2,8-1,8-2,9-1,10-1}.js` | ported, passing | The name/message combinations of `toString`. |
| `isError/{errors,error-subclass,primitives,bigints,non-error-objects,fake-errors}.js` | ported, passing | Arms for `EvalError`/`ReferenceError`/`URIError`/`AggregateError`/`SuppressedError`, `undefined`/no-argument calls, and constructors passed as values are dropped. `fake-errors` becomes an object literal with Error's fields. |
| `cause_property.js` | ported, `expect-error` | `new Error(message, { cause })` isn't in §1.8. Pinned to ``constructor of `Error` expects 1 argument(s), got 2``. |
| `the-initial-value-of-errorprototypemessage-is-the-empty-string.js` | ported, `expect-error` | `Error('a')` without `new`: a class isn't callable. Pinned to ``` `Error` is a class, not a value ```. |
| `prototype/toString/S15.11.4.4_A2.js` (`toString()` is not `undefined`) | not ported | Redundant with `15.11.4.4-6-2`. |
| `constructor.js`, `cause_abrupt.js` | rejected | Message `toString` coercion and `cause` getter traps. Originals in `rejected/`. |
| `error-message-tostring-symbol.js`, `error-message-tostring-toprimitive.js`, `prototype/toString/tostring-message-throws-{symbol,toprimitive}.js`, `isError/symbols.js` | rejected | Symbol / `Symbol.toPrimitive` coercion of the message. Representative original in `rejected/`. |
| `instance-prototype.js`, `internal-prototype.js`, `prototype/S15.11.3.1_A{1,2,3,4}_T1.js`, `prototype/S15.11.4_A{1,2,3,4}.js`, `prototype/constructor/S15.11.4.1_A1_T2.js`, `prototype/no-error-data.js` | rejected | `Error.prototype`, `isPrototypeOf`, and calling or constructing the prototype. There are no prototypes, and classes aren't values. Representative originals in `rejected/`. |
| `tostring-1.js`, `tostring-2.js` | rejected | They reassign `Error.prototype.toString`. `tostring-2` also calls `Error()` without `new`. The unmodified `toString` arm is covered by `15.11.4.4-6-1`. Original in `rejected/`. |
| `prototype/toString/{undefined-props,invalid-receiver,called-as-function,tostring-get-throws}.js` | rejected | `Error.prototype.toString.call` on other receivers and throwing getters. There's no this-transfer and there are no getter traps. Representative originals in `rejected/`. |
| `prototype/stack/**` (35 files) | rejected | The `Error.prototype.stack` accessor (proposal). Errors have no `stack` field, and these tests exercise accessors, proxies, realms, and `delete`. Representative original in `rejected/`. |
| `proto-from-ctor-realm.js`, `isError/{errors,non-error-objects}-other-realm.js` | rejected | No realms. Representative original in `rejected/`. |
| `length.js`, `name.js`, `prop-desc.js`, `is-a-constructor.js`, `prototype/{constructor,message,name}/prop-desc.js`, `prototype/toString/{length,name,not-a-constructor,prop-desc}.js`, `isError/{is-a-constructor,name,prop-desc}.js` | rejected | Blanket rules (property descriptors, `IsConstructor` reflection). |
