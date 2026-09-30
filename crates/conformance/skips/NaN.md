# NaN

Source: `test/built-ins/NaN/**` (6 files). `NaN` is a prelude constant
(spec §1.6), not a property of a global object, so only the value tests port.

Ported: 2 (1 passing, 1 `expect-error` pin), under `cases/NaN/`.
Representative rejected originals under `rejected/NaN/`.

| Pattern | Disposition | Reason |
|:--------|:------------|:-------|
| `S15.1.1.1_A1` | ported, passing | `NaN !== NaN`. |
| `S15.1.1.1_A2_T2` (`NaN = true`) | ported, `expect-error` | Writing a prelude constant is a compile error (``cannot assign to const binding `NaN` ``); the standard keeps the global non-writable by silently ignoring the sloppy-mode write (the test's `noStrict` flag) or throwing in strict mode. |
| `15.1.1.1-0` (descriptor attributes), `prop-desc.js` | rejected | Property descriptors on the global object (blanket rule). Original of `15.1.1.1-0.js` in `rejected/`. |
| `S15.1.1.1_A3_T2` (`delete NaN`) | rejected | The `delete` operator is excluded by design (compile error), and there is no global object to delete from. Original in `rejected/`. |
| `S15.1.1.1_A4` (for-in over `this`) | rejected | Enumerates the global object through top-level `this` — no global object, no enumerability attributes. Original in `rejected/`. |
