# NativeErrors

Source: `test/built-ins/NativeErrors/**` (94 files). Of the six native error
classes, `RangeError`, `ReferenceError`, `SyntaxError`, `TypeError`, and
`URIError` exist here as built-in `Error` subclasses. `EvalError` does not. Each class directory has the same 15 files, and most of
them test constructor and prototype objects.

Ported: 11 (10 passing, 1 `expect-fail` known gap). Representative rejected
originals under `rejected/NativeErrors/`.

The restored cause case unrolls the pinned upstream `nativeErrors` list over
`Error` and all five supported native error classes, because classes are not
values. `EvalError` is excluded. Older ports cover the original three classes.

| Pattern | Disposition | Reason |
|:--------|:------------|:-------|
| `message_property_native_error.js` | ported, passing | Value and write checks kept; enumerable/configurable dropped. |
| `{Range,Syntax,Type}Error/instance-proto.js` | ported, passing | "Instance's prototype is `X.prototype`" becomes `instanceof X` on an `unknown`-widened value. |
| `{Range,Syntax,Type}Error/prototype/proto.js` | ported, passing | "`X.prototype` inherits from `Error.prototype`" becomes `instanceof Error`, plus `catch (e: Error)` binding the subclass. |
| `{Range,Syntax,Type}Error/prototype/name.js` | ported, passing | The `name` value is read through an instance, with `toString()`. Descriptor checks dropped. |
| `cause_property_native_error.js` | ported, `expect-fail` (SUB-1830) | Retains own-property and cause-value checks for supplied, omitted, explicit `undefined`, and empty options across the supported constructors. Omitted causes still occupy own fields. Descriptor flags are excluded; object-valued SameValue is structural. |
| `{Range,Syntax,Type}Error/prototype/message.js` (the prototype's `message` is `""`) | not ported | The empty constructor message is covered by interpreter fixtures; prototype reflection remains excluded. |
| `{Range,Syntax,Type}Error/is-error-object.js` (`[object Error]` brand) | not ported | The `[[ErrorData]]` brand is `Error.isError` here, already covered by `Error/isError/errors.ts`. |
| `EvalError/**` | rejected | `EvalError` is unavailable. Representative original remains in `rejected/`. |
| `{ReferenceError,URIError}/**` | not ported | These classes now exist and are included in the cause case; their separate prototype/constructor inventories have not been ported. |
| `{Range,Syntax,Type}Error/constructor.js` (`typeof X === 'function'`) | rejected | A class isn't a value, and `typeof` is a narrowing guard only. Original in `rejected/`. |
| `{Range,Syntax,Type}Error/{proto,prototype}.js`, `{Range,Syntax,Type}Error/prototype/{constructor,not-error-object}.js` | rejected | Constructor and prototype objects (`Object.getPrototypeOf`, `X.prototype`, `Object.prototype.toString.call`). There are no prototypes. Representative originals in `rejected/`. |
| `nativeerror-tostring-message-throws-{symbol,toprimitive}.js` | rejected | Symbol / `Symbol.toPrimitive` message coercion, and calling classes without `new`. Representative original in `rejected/`. |
| `{Range,Syntax,Type}Error/proto-from-ctor-realm.js` | rejected | No realms (blanket). |
| `{Range,Syntax,Type}Error/{is-a-constructor,length,name,prop-desc}.js` | rejected | Blanket rules (property descriptors, `IsConstructor` reflection). |
