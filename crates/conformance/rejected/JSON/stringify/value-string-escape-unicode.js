// rejected: strings are well-formed Unicode — lone surrogate code units are unrepresentable (the lexer rejects a lone uD834 escape)
// Copyright (c) 2018 Ecma International.  All rights reserved.
// This code is governed by the BSD license found in the LICENSE file.

/*---
esid: sec-quotejsonstring
description: >
    JSON.stringify strings containing surrogate code units
features: [well-formed-json-stringify]
---*/

assert.sameValue(JSON.stringify("\uD834"), '"\\ud834"',
  'JSON.stringify("\\uD834")');
assert.sameValue(JSON.stringify("\uDF06"), '"\\udf06"',
  'JSON.stringify("\\uDF06")');

assert.sameValue(JSON.stringify("\uD834\uDF06"), '"𝌆"',
  'JSON.stringify("\\uD834\\uDF06")');
assert.sameValue(JSON.stringify("\uD834\uD834\uDF06\uD834"), '"\\ud834𝌆\\ud834"',
  'JSON.stringify("\\uD834\\uD834\\uDF06\\uD834")');
assert.sameValue(JSON.stringify("\uD834\uD834\uDF06\uDF06"), '"\\ud834𝌆\\udf06"',
  'JSON.stringify("\\uD834\\uD834\\uDF06\\uDF06")');
assert.sameValue(JSON.stringify("\uDF06\uD834\uDF06\uD834"), '"\\udf06𝌆\\ud834"',
  'JSON.stringify("\\uDF06\\uD834\\uDF06\\uD834")');
assert.sameValue(JSON.stringify("\uDF06\uD834\uDF06\uDF06"), '"\\udf06𝌆\\udf06"',
  'JSON.stringify("\\uDF06\\uD834\\uDF06\\uDF06")');

assert.sameValue(JSON.stringify("\uDF06\uD834"), '"\\udf06\\ud834"',
  'JSON.stringify("\\uDF06\\uD834")');
assert.sameValue(JSON.stringify("\uD834\uDF06\uD834\uD834"), '"𝌆\\ud834\\ud834"',
  'JSON.stringify("\\uD834\\uDF06\\uD834\\uD834")');
assert.sameValue(JSON.stringify("\uD834\uDF06\uD834\uDF06"), '"𝌆𝌆"',
  'JSON.stringify("\\uD834\\uDF06\\uD834\\uDF06")');
assert.sameValue(JSON.stringify("\uDF06\uDF06\uD834\uD834"), '"\\udf06\\udf06\\ud834\\ud834"',
  'JSON.stringify("\\uDF06\\uDF06\\uD834\\uD834")');
assert.sameValue(JSON.stringify("\uDF06\uDF06\uD834\uDF06"), '"\\udf06\\udf06𝌆"',
  'JSON.stringify("\\uDF06\\uDF06\\uD834\\uDF06")');
