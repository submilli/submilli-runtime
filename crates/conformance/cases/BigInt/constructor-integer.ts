// test262: test/built-ins/BigInt/constructor-integer.js

function main(): void {
  assertSameValue(
    BigInt(Number.MAX_SAFE_INTEGER), 9007199254740991n,
    "BigInt(Number.MAX_SAFE_INTEGER) === 9007199254740991n",
  );
  assertSameValue(
    BigInt(-Number.MAX_SAFE_INTEGER), -9007199254740991n,
    "BigInt(-Number.MAX_SAFE_INTEGER) === -9007199254740991n",
  );
  assertSameValue(
    BigInt(Number.MAX_SAFE_INTEGER + 1), 9007199254740992n,
    "BigInt(Number.MAX_SAFE_INTEGER + 1) === 9007199254740992n",
  );
  assertSameValue(
    BigInt(-Number.MAX_SAFE_INTEGER - 1), -9007199254740992n,
    "BigInt(-Number.MAX_SAFE_INTEGER - 1) === -9007199254740992n",
  );
  assertSameValue(
    BigInt(Number.MAX_SAFE_INTEGER + 2), 9007199254740992n,
    "BigInt(Number.MAX_SAFE_INTEGER + 2) === 9007199254740992n",
  );
  assertSameValue(
    BigInt(-Number.MAX_SAFE_INTEGER - 2), -9007199254740992n,
    "BigInt(-Number.MAX_SAFE_INTEGER - 2) === -9007199254740992n",
  );
  assertSameValue(
    BigInt(Number.MAX_SAFE_INTEGER + 3), 9007199254740994n,
    "BigInt(Number.MAX_SAFE_INTEGER + 3) === 9007199254740994n",
  );
  assertSameValue(
    BigInt(-Number.MAX_SAFE_INTEGER - 3), -9007199254740994n,
    "BigInt(-Number.MAX_SAFE_INTEGER - 3) === -9007199254740994n",
  );
}
