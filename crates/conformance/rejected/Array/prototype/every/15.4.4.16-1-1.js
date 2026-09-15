// rejected: this-coercion — Array.prototype methods cannot be applied to undefined/array-like receivers; methods exist only on real arrays
// Copyright (c) 2012 Ecma International.  All rights reserved.
// This code is governed by the BSD license found in the LICENSE file.

/*---
esid: sec-array.prototype.every
description: Array.prototype.every applied to undefined throws a TypeError
---*/


assert.throws(TypeError, function() {
  Array.prototype.every.call(undefined); // TypeError is thrown if value is undefined
});
