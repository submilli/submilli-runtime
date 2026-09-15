// test262: test/built-ins/parseInt/S15.1.2.2_A6.1_T1.js

function main(): void {
  for (let i = 2; i <= 36; i++) {
    assertSameValue(parseInt("10$1", i), i, 'parseInt("10$1", i) must return the value of i');
  }
}
