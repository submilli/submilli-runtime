// test262: test/built-ins/Math/ceil/S15.8.2.6_A7.js

function main(): void {
  for (let i = -1000; i < 1000; i++) {
    const x: number = i / 10.0;
    assertSameValue(Math.ceil(x), -Math.floor(-x), "Math.ceil(i / 10.0) must return -Math.floor(-x)");
  }
}
