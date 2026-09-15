// rejected: reduce without an initial value is a compile-time arity error by design (spec.md §1.2) — the empty-array TypeError path cannot exist; pinned by cases/Array/divergence/reduce-requires-initial-value.ts
// Copyright (c) 2012 Ecma International.  All rights reserved.
// This code is governed by the BSD license found in the LICENSE file.

/*---
esid: sec-array.prototype.reduce
description: >
    Array.prototype.reduce throws TypeError when Array is empty and
    initialValue is not present
---*/

function callbackfn(prevVal, curVal, idx, obj)
{}

var arr = new Array(10);
assert.throws(TypeError, function() {
  arr.reduce(callbackfn);
});
