// test262: test/built-ins/decodeURIComponent/S15.1.3.2_A1.12_T2.js
// URIError is erased to the base Error (README: no error subclasses); the
// "throws URIError" checks become "throws".

const interval: number[][] = [
  [0x00, 0x2F],
  [0x3A, 0x40],
  [0x47, 0x60],
  [0x67, 0xFFFF],
];

function main(): void {
  let result = true;
  for (let indexI = 0; indexI < interval.length; indexI++) {
    // Math.floor of these integer bounds is a no-op; a loop counter seeded
    // straight from an array element runs ~50x slower in the runtime.
    const low = Math.floor(interval[indexI][0]);
    const high = Math.floor(interval[indexI][1]);
    for (let indexJ = low; indexJ <= high; indexJ++) {
      let threw = false;
      try {
        decodeURIComponent("%F0" + "%A0%" + String.fromCharCode(indexJ, indexJ) + "%A0");
      } catch (e: Error) {
        threw = true;
      }
      if (!threw) {
        result = false;
      }
    }
  }
  assert(result === true, "#1: If B = 11110xxx (n = 4) and (string.charAt(k + 7) and string.charAt(k + 8)) do not represent hexadecimal digits, throw URIError");
}
