// test262: test/built-ins/decodeURI/S15.1.3.1_A2.1_T1.js
//
// The try/catch around the decode becomes a separate throws check; the
// "differs" check then decodes again outside it.

// test262 harness helpers (harness/decimalToHexString.js).
const HEX: string = "0123456789ABCDEF";

function decimalToHexString(n: number): string {
  let s = "";
  let v = n;
  while (v > 0) {
    s = HEX.charAt(v % 16) + s;
    v = Math.floor(v / 16);
  }
  while (s.length < 4) {
    s = "0" + s;
  }
  return s;
}

function main(): void {
  for (let indexI = 0; indexI <= 65535; indexI++) {
    if (indexI !== 0x25) {
      const str = String.fromCharCode(indexI);
      let threw = false;
      try {
        decodeURI(str);
      } catch (e: Error) {
        threw = true;
      }
      if (threw) {
        assert(false, "#" + decimalToHexString(indexI) + " throws");
      }
      if (decodeURI(str) !== str) {
        assert(false, "#" + decimalToHexString(indexI) + " differs");
      }
    }
  }
}
