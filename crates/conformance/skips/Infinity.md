# Infinity

Source: `test/built-ins/Infinity/**` (6 files). `Infinity` is a prelude constant
(spec §1.6), not a property of a global object, so only the value tests port.

Ported: 2 (1 passing, 1 `expect-error` pin), under `cases/Infinity/`.
Representative rejected originals under `rejected/Infinity/`.

| Pattern | Disposition | Reason |
|:--------|:------------|:-------|
| `S15.1.1.2_A1` | ported, passing | `typeof Infinity === "number"`, `isFinite`/`isNaN` of it, and SameValue with `Number.POSITIVE_INFINITY`. |
| `S15.1.1.2_A2_T2` (`Infinity = true`) | ported, `expect-error` | Writing a prelude constant is a compile error (``cannot assign to const binding `Infinity` ``); the standard keeps the global non-writable by silently ignoring the sloppy-mode write (the test's `noStrict` flag) or throwing in strict mode. |
| `15.1.1.2-0` (descriptor attributes), `prop-desc.js` | rejected | Property descriptors on the global object (blanket rule). Original of `15.1.1.2-0.js` in `rejected/`. |
| `S15.1.1.2_A3_T2` (`delete Infinity`) | rejected | The `delete` operator is excluded by design (compile error), and there is no global object to delete from. Original in `rejected/`. |
| `S15.1.1.2_A4` (for-in over `this`) | rejected | Enumerates the global object through top-level `this` — no global object, no enumerability attributes. Original in `rejected/`. |
