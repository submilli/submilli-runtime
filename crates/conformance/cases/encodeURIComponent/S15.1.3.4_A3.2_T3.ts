// test262: test/built-ins/encodeURIComponent/S15.1.3.4_A3.2_T3.js

const uriMark: string[] = ["-", "_", ".", "!", "~", "*", "'", "(", ")"];

function main(): void {
  for (let indexC = 0; indexC < uriMark.length; indexC++) {
    const str = uriMark[indexC];
    assertSameValue(encodeURIComponent(str), str, "#" + String(indexC + 1) + ": unescapedURISet containing" + str);
  }
}
