# NativeErrors

Source: `test/built-ins/NativeErrors/**` (94 files). Of the six native error
classes, `RangeError`, `SyntaxError`, and `TypeError` exist here as built-in
`Error` subclasses (spec.md §1.8). `EvalError`, `ReferenceError`, and
`URIError` don't. Each class directory has the same 15 files, and most of
them test constructor and prototype objects.

Ported: 11 (10 passing, 1 `expect-error` pin). Representative rejected
originals under `rejected/NativeErrors/`.

Where the test262 harness iterates `nativeErrors`, the port unrolls the loop
over the three classes that exist, because classes aren't values.

| Pattern | Disposition | Reason |
|:--------|:------------|:-------|
| `message_property_native_error.js` | ported, passing | Value and write checks kept; enumerable/configurable dropped. |
| `{Range,Syntax,Type}Error/instance-proto.js` | ported, passing | "Instance's prototype is `X.prototype`" becomes `instanceof X` on an `unknown`-widened value. |
| `{Range,Syntax,Type}Error/prototype/proto.js` | ported, passing | "`X.prototype` inherits from `Error.prototype`" becomes `instanceof Error`, plus `catch (e: Error)` binding the subclass. |
| `{Range,Syntax,Type}Error/prototype/name.js` | ported, passing | The `name` value is read through an instance, with `toString()`. Descriptor checks dropped. |
| `cause_property_native_error.js` | ported, `expect-error` | The `{ cause }` option isn't in §1.8. Pinned to ``constructor of `RangeError` expects 1 argument(s), got 2``. |
| `{Range,Syntax,Type}Error/prototype/message.js` (the prototype's `message` is `""`) | not ported | The value is only observable through `new X()` with no message, and the constructor requires one (adaptation rule 5). |
| `{Range,Syntax,Type}Error/is-error-object.js` (`[object Error]` brand) | not ported | The `[[ErrorData]]` brand is `Error.isError` here, already covered by `Error/isError/errors.ts`. |
| `{EvalError,ReferenceError,URIError}/**` (45 files) | rejected | These classes don't exist. The built-in subclasses are `RangeError`, `TypeError`, `SyntaxError`, and `PermissionDeniedError`. Representative original in `rejected/`. |
| `{Range,Syntax,Type}Error/constructor.js` (`typeof X === 'function'`) | rejected | A class isn't a value, and `typeof` is a narrowing guard only. Original in `rejected/`. |
| `{Range,Syntax,Type}Error/{proto,prototype}.js`, `{Range,Syntax,Type}Error/prototype/{constructor,not-error-object}.js` | rejected | Constructor and prototype objects (`Object.getPrototypeOf`, `X.prototype`, `Object.prototype.toString.call`). There are no prototypes. Representative originals in `rejected/`. |
| `nativeerror-tostring-message-throws-{symbol,toprimitive}.js` | rejected | Symbol / `Symbol.toPrimitive` message coercion, and calling classes without `new`. Representative original in `rejected/`. |
| `{Range,Syntax,Type}Error/proto-from-ctor-realm.js` | rejected | No realms (blanket). |
| `{Range,Syntax,Type}Error/{is-a-constructor,length,name,prop-desc}.js` | rejected | Blanket rules (property descriptors, `IsConstructor` reflection). |
