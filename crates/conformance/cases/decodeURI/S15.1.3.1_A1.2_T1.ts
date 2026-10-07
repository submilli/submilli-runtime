// test262: test/built-ins/decodeURI/S15.1.3.1_A1.2_T1.js

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
        decodeURI("%" + String.fromCharCode(indexJ) + "1");
      } catch (e) {
        threw = e instanceof URIError;
      }
      if (!threw) {
        result = false;
      }
    }
  }
  assert(result === true, "#1: If string.charAt(k+1) does not represent hexadecimal digits, throw URIError");
}
