// test262: test/built-ins/Array/prototype/push/S15.4.4.7_A1_T2.js
// Source: https://github.com/tc39/test262/blob/d53ebc8b1931d9f1541e43d59650776e4b766248/test/built-ins/Array/prototype/push/S15.4.4.7_A1_T2.js
// unknown[] preserves all original element kinds. push(0) initializes the dense
// array because writing past its end is unsupported. Error represents Test262Error;
// String(...) makes failure-message coercion explicit. The initial check uses
// assert so TypeScript does not keep a literal length of 0 across the pushes.

function main(): void {
  const x: unknown[] = [];
  assert(x.length === 0, '#1: x = []; x.length === 0. Actual: ' + String(x.length));

  x.push(0);
  const push = x.push(true, Number.POSITIVE_INFINITY, "NaN", "1", -1);
  if (push !== 6) {
    throw new Error('#2: x = []; x[0] = 0; x.push(true, Number.POSITIVE_INFINITY, "NaN", "1", -1) === 6. Actual: ' + String(push));
  }

  if (x[0] !== 0) {
    throw new Error('#3: x = []; x[0] = 0; x.push(true, Number.POSITIVE_INFINITY, "NaN", "1", -1); x[0] === 0. Actual: ' + String(x[0]));
  }

  if (x[1] !== true) {
    throw new Error('#4: x = []; x[0] = 0; x.push(true, Number.POSITIVE_INFINITY, "NaN", "1", -1); x[1] === true. Actual: ' + String(x[1]));
  }

  if (x[2] !== Number.POSITIVE_INFINITY) {
    throw new Error('#5: x = []; x[0] = 0; x.push(true, Number.POSITIVE_INFINITY, "NaN", "1", -1); x[2] === Number.POSITIVE_INFINITY. Actual: ' + String(x[2]));
  }

  if (x[3] !== "NaN") {
    throw new Error('#6: x = []; x[0] = 0; x.push(true, Number.POSITIVE_INFINITY, "NaN", "1", -1); x[3] === "NaN". Actual: ' + String(x[3]));
  }

  if (x[4] !== "1") {
    throw new Error('#7: x = []; x[0] = 0; x.push(true, Number.POSITIVE_INFINITY, "NaN", "1", -1); x[4] === "1". Actual: ' + String(x[4]));
  }

  if (x[5] !== -1) {
    throw new Error('#8: x = []; x[0] = 0; x.push(true, Number.POSITIVE_INFINITY, "NaN", "1", -1); x[5] === -1. Actual: ' + String(x[5]));
  }

  if (x.length !== 6) {
    throw new Error('#9: x = []; x[0] = 0; x.push(true, Number.POSITIVE_INFINITY, "NaN", "1", -1); x.length === 6. Actual: ' + String(x.length));
  }
}
