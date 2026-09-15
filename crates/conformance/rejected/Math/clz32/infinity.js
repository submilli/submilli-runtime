// rejected: JS ToUint32(Infinity) is 0 (clz32 = 32); our saturating conversion clamps +Infinity to 2^32-1 (clz32 = 0) — documented divergence (spec.md 1.10)
// Copyright (C) 2016 The V8 Project authors. All rights reserved.
// This code is governed by the BSD license found in the LICENSE file.

/*---
esid: sec-math.clz32
description: >
  Return 32 if x is Infinity or -Infinity
info: |
  Math.clz32 ( x )

  1. Let n be ToUint32(x).
  2. Let p be the number of leading zero bits in the 32-bit binary representation of n.
  3. Return p.

  7.1.6 ToUint32 ( argument )

  [...]
  2. If number is NaN, +0, -0, +∞, or -∞, return +0.
  [...]
---*/

assert.sameValue(Math.clz32(Infinity), 32, "Infinity");
assert.sameValue(Math.clz32(-Infinity), 32, "-Infinity");
