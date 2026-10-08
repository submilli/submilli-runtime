// test262: test/built-ins/String/prototype/split/separator-regexp.js
// The rows built on Annex B escapes (`\0`, `\k<x>` without groups, `\X`, `\x`,
// `\c`) and on `[]`/`[^]` are dropped; the regex engine rejects them.

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
  assertCompareArray("x".split(/()/), ["x"], "\"x\".split(/()/) must return [\"x\"]");
  assertCompareArray("x".split(/(...)/), ["x"], "\"x\".split(/(...)/) must return [\"x\"]");
  assertCompareArray("x".split(/(|)/), ["x"], "\"x\".split(/(|)/) must return [\"x\"]");
  assertCompareArray("x".split(/[.-.]/), ["x"], "\"x\".split(/[.-.]/) must return [\"x\"]");
  assertCompareArray("x".split(/\b/), ["x"], "\"x\".split(/\\b/) must return [\"x\"]");
  assertCompareArray("x".split(/\B/), ["x"], "\"x\".split(/\\B/) must return [\"x\"]");
  assertCompareArray("x".split(/\d/), ["x"], "\"x\".split(/\\d/) must return [\"x\"]");
  assertCompareArray("x".split(/\D/), ["", ""], "\"x\".split(/\\D/) must return [\"\", \"\"]");
  assertCompareArray("x".split(/\n/), ["x"], "\"x\".split(/\\n/) must return [\"x\"]");
  assertCompareArray("x".split(/\r/), ["x"], "\"x\".split(/\\r/) must return [\"x\"]");
  assertCompareArray("x".split(/\s/), ["x"], "\"x\".split(/\\s/) must return [\"x\"]");
  assertCompareArray("x".split(/\S/), ["", ""], "\"x\".split(/\\S/) must return [\"\", \"\"]");
  assertCompareArray("x".split(/\v/), ["x"], "\"x\".split(/\\v/) must return [\"x\"]");
  assertCompareArray("x".split(/\w/), ["", ""], "\"x\".split(/\\w/) must return [\"\", \"\"]");
  assertCompareArray("x".split(/\W/), ["x"], "\"x\".split(/\\W/) must return [\"x\"]");
  assertCompareArray("x".split(/\xA0/), ["x"], "\"x\".split(/\\xA0/) must return [\"x\"]");
  assertCompareArray("x".split(/[\b]/), ["x"], "\"x\".split(/[\\b]/) must return [\"x\"]");
}
