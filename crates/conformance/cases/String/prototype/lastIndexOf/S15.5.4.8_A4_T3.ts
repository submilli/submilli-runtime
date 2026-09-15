// test262: test/built-ins/String/prototype/lastIndexOf/S15.5.4.8_A4_T3.js
// The coerced arguments collapse to their results: searchString "AB" and a
// position that coerces to NaN, which means "search the whole string".

function main(): void {
  assertSameValue("ABBABABAB".lastIndexOf("AB"), 7, 'lastIndexOf finds the rightmost occurrence');
}
