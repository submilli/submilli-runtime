// test262: test/built-ins/String/prototype/split/separator-regexp.js
// expect-fail: ECMA-262 SplitMatch skips empty matches at the current position, so "x".split(/^/) and "x".split(/(?:)/) are ["x"]; the engine's split treats them as real separators and yields leading/trailing empty strings
// The capture-group row (/()/ — capture splicing) is covered by the divergence
// pin under cases/RegExp/divergence/split-no-capture-insertion.ts.

function main(): void {
  assertCompareArray("x".split(/^/), ["x"], "\"x\".split(/^/) must return [\"x\"]");
  assertCompareArray("x".split(/$/), ["x"], "\"x\".split(/$/) must return [\"x\"]");
  assertCompareArray("x".split(/.?/), ["", ""], "\"x\".split(/.?/) must return [\"\", \"\"]");
  assertCompareArray("x".split(/.*/), ["", ""], "\"x\".split(/.*/) must return [\"\", \"\"]");
  assertCompareArray("x".split(/.+/), ["", ""], "\"x\".split(/.+/) must return [\"\", \"\"]");
  assertCompareArray("x".split(/.*?/), ["x"], "\"x\".split(/.*?/) must return [\"x\"]");
  assertCompareArray("x".split(/.{1}/), ["", ""], "\"x\".split(/.{1}/) must return [\"\", \"\"]");
  assertCompareArray("x".split(/.{1,}/), ["", ""], "\"x\".split(/.{1,}/) must return [\"\", \"\"]");
  assertCompareArray("x".split(/.{1,2}/), ["", ""], "\"x\".split(/.{1,2}/) must return [\"\", \"\"]");
  assertCompareArray("x".split(/./), ["", ""], "\"x\".split(/./) must return [\"\", \"\"]");
  assertCompareArray("x".split(/(?:)/), ["x"], "\"x\".split(/(?:)/) must return [\"x\"]");
}
