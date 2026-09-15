// test262: test/built-ins/Array/of/creates-a-new-array-from-arguments.js
// Adapted: undefined -> null; the mixed null/boolean argument list uses an
// explicit union element type.

type BooleanOrNull = boolean | null;

function main(): void {
  const a1 = Array.of("Mike", "Rick", "Leo");
  assertSameValue(a1.length, 3, "The value of a1.length is expected to be 3");
  assertSameValue(a1[0], "Mike", 'The value of a1[0] is expected to be "Mike"');
  assertSameValue(a1[1], "Rick", 'The value of a1[1] is expected to be "Rick"');
  assertSameValue(a1[2], "Leo", 'The value of a1[2] is expected to be "Leo"');

  const a2: BooleanOrNull[] = Array.of(null, false, null, null);
  assertSameValue(a2.length, 4, "The value of a2.length is expected to be 4");
  assertSameValue(a2[0], null, "The value of a2[0] is expected to equal null");
  assertSameValue(a2[1], false, "The value of a2[1] is expected to be false");
  assertSameValue(a2[2], null, "The value of a2[2] is expected to be null");
  assertSameValue(a2[3], null, "The value of a2[3] is expected to equal null");

  const a3: number[] = Array.of();
  assertSameValue(a3.length, 0, "The value of a3.length is expected to be 0");
}
