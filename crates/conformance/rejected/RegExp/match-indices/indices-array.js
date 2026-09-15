// rejected: d flag (hasIndices) and the match .indices array are deferred (docs/regex.md, docs/ecma-262-gaps.md §22)
// Copyright 2019 Ron Buckton. All rights reserved.
// This code is governed by the BSD license found in the LICENSE file.

/*---
description: The "indices" property is an Array.
esid: sec-makeindicesarray
features: [regexp-match-indices]
info: |
  MakeIndicesArray ( S, indices, groupNames, hasGroups )
    6. Set _A_ to ! ArrayCreate(_n_).
---*/

let match = /a/d.exec("a");
let indices = match.indices;

// `indices` is an array
assert.sameValue(Object.getPrototypeOf(indices), Array.prototype);
assert(Array.isArray(indices));
