// rejected: typeof RangeError === 'function' — a class is not a value, and typeof is a narrowing guard only
// Copyright (C) 2015 André Bargull. All rights reserved.
// This code is governed by the BSD license found in the LICENSE file.

/*---
es6id: 19.5.6.1
description: >
  RangeError is a constructor function.
---*/

assert.sameValue(typeof RangeError, 'function', 'typeof RangeError is "function"');
