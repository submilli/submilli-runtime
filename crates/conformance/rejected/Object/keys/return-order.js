// rejected: expects integer-keys-then-insertion enumeration order; our objects have no insertion order — Object.keys enumerates in canonical sorted order (see cases/Object/divergence/keys-canonical-sorted-order.ts). Also relies on delete and dynamic numeric keys.
// Copyright 2019 Kevin Gibbons. All rights reserved.
// This code is governed by the BSD license found in the LICENSE file.

/*---
esid: sec-object.keys
description: Object.keys enumeration order
features: [for-in-order]
includes: [compareArray.js]
---*/

var o = {
  p1: 'p1',
  p2: 'p2',
  p3: 'p3',
};
o.p4 = 'p4';

o[2] = '2';
o[0] = '0';
o[1] = '1';

delete o.p1;
delete o.p3;
o.p1 = 'p1';

var actual = Object.keys(o);

var expected = ['0', '1', '2', 'p2', 'p4', 'p1'];

assert.compareArray(actual, expected);
