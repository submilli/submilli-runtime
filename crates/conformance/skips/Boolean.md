# Boolean

Source: `test/built-ins/Boolean/**`. The primitive conversion ports use `!!value`
as an adapter for `Boolean(value)`, including undefined and void completion
values. The Boolean callable, wrapper objects and prototype manipulation remain
outside these ports.

| Pattern | Disposition | Reason |
|:--------|:------------|:-------|
| `S15.6.1.1_A1_T2/T3` (`Boolean(number)`, `Boolean(string)`) | ported | Primitive number and string conversions. |
| `S9.2_A3_T1` (`Boolean(boolean)` is identity) | ported | Boolean conversion preserves boolean values. |
| `prototype/toString/S15.6.4.2_A1_T1` | ported, passing | Distilled to primitives: `true.toString()` / `false.toString()`; wrapper receivers dropped. |
| `prototype/toString/S15.6.4.2_A1_T2` (toString with args) | ported, `expect-error` | JS ignores extra arguments; typed signatures reject them (``method `toString` expects 0 argument(s)``) — same divergence as TypeScript. |
| `S15.6.1.1_A1_T1`, `S15.6.1.1_A2`, `S9.2_A1/A2/A4/A5` (more `Boolean()` / ToBoolean vectors) | not ported | Additional conversion variants are redundant with the ported cases; eval remains unsupported. |
| `S15.6.1.1_A1_T4` (`Boolean(undefined)`, `void 0`) | ported | Undefined, void expressions, null and void function completion convert to false. |
| `S15.6.2.1_A1–A4` (`new Boolean` constructor) | rejected | No primitive wrapper objects. Representative original in `rejected/`. |
| `S15.6.3_A1–A3`, `prototype/S15.6.3.1_*`, `prototype/S15.6.4_A2`, `prototype/constructor/*`, `proto-from-ctor-realm.js` | rejected | Prototype chain / `Boolean.prototype` / realms — no prototypes. Representative original in `rejected/`. |
| `prototype/valueOf/*` | rejected | `valueOf` boxing — no wrapper objects. Representative original in `rejected/`. |
| `prototype/toString/S15.6.4.2_A2_T*` (toString not generic, `this`-transfer) | rejected | Requires wrapper objects and dynamic method assignment. |
| `S9.2_A6_T1` (ToBoolean(object) is true) | rejected | Truthy coercion of objects via wrapper constructors. Original in `rejected/`. |
| `symbol-coercion.js` | rejected | No `Symbol`. Original in `rejected/`. |
| `prop-desc.js`, `is-a-constructor.js`, `prototype/toString/{length,name,not-a-constructor}.js`, `prototype/valueOf/{length,name,not-a-constructor}.js` | rejected | Blanket rules (property descriptors, `IsConstructor` reflection). |
