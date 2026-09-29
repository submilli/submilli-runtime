// test262: test/built-ins/decodeURI/S15.1.3.1_A3_T3.js

function main(): void {
  assertSameValue(decodeURI("%3B%2F%3F%3A%40%26%3D%2B%24%2C%23"), "%3B%2F%3F%3A%40%26%3D%2B%24%2C%23", "#1");
  assertSameValue(decodeURI("%3b%2f%3f%3a%40%26%3d%2b%24%2c%23"), "%3b%2f%3f%3a%40%26%3d%2b%24%2c%23", "#2");
}
