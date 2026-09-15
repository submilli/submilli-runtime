// test262: test/built-ins/Number/S9.3.1_A32.js

function main(): void {
  assertSameValue(
    Number("1234567890.1234567890"),
    1234567890.1234567890,
    'Number("1234567890.1234567890") must return 1234567890.1234567890',
  );

  assertSameValue(
    Number("1234567890.1234567890"),
    1234567890.1234567000,
    'Number("1234567890.1234567890") must return 1234567890.1234567000',
  );

  assertNotSameValue(
    Number("1234567890.1234567890"),
    1234567890.123456,
    'The value of Number("1234567890.1234567890") is not 1234567890.123456',
  );

  assertSameValue(
    Number("0.12345678901234567890"),
    0.123456789012345678,
    'Number("0.12345678901234567890") must return 0.123456789012345678',
  );
}
