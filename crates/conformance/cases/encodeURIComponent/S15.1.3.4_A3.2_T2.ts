// test262: test/built-ins/encodeURIComponent/S15.1.3.4_A3.2_T2.js

const DecimalDigit: string[] = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];

function main(): void {
  for (let indexC = 0; indexC < DecimalDigit.length; indexC++) {
    const str = DecimalDigit[indexC];
    assertSameValue(encodeURIComponent(str), str, "#" + String(indexC + 1) + ": unescapedURISet containing" + str);
  }
}
