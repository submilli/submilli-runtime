// test262: test/built-ins/encodeURI/S15.1.3.3_A3.2_T3.js

const uriMark: string[] = ["-", "_", ".", "!", "~", "*", "'", "(", ")"];

function main(): void {
  for (let indexC = 0; indexC < uriMark.length; indexC++) {
    const str = uriMark[indexC];
    assertSameValue(encodeURI(str), str, "#" + String(indexC + 1) + ": unescapedURISet containing" + str);
  }
}
