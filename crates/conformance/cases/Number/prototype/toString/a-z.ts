// test262: test/built-ins/Number/prototype/toString/a-z.js

function main(): void {
  for (let radix = 11; radix <= 36; radix++) {
    for (let i = 10; i < radix; i++) {
      assertSameValue(i.toString(radix), String.fromCharCode(i + 87));
    }
  }
}
