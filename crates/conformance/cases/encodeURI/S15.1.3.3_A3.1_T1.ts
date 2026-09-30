// test262: test/built-ins/encodeURI/S15.1.3.3_A3.1_T1.js

const uriReserved: string[] = [";", "/", "?", ":", "@", "&", "=", "+", "$", ","];

function main(): void {
  for (let indexC = 0; indexC < uriReserved.length; indexC++) {
    const str = uriReserved[indexC];
    assertSameValue(encodeURI(str), str, "#" + String(indexC + 1) + ": unescapedURISet containing" + str);
  }
}
