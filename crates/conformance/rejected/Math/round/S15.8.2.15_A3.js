// rejected: Math.round is floor(x + 0.5) and returns +0 for -0 — documented sign-of-zero divergence (spec.md 1.10)
// Copyright 2009 the Sputnik authors.  All rights reserved.
// This code is governed by the BSD license found in the LICENSE file.

/*---
info: If x is -0, Math.round(x) is -0
es5id: 15.8.2.15_A3
description: Checking if Math.round(x) equals to -0, where x is -0
---*/

assert.sameValue(Math.round(-0), -0);
