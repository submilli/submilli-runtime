// test262: test/built-ins/encodeURIComponent/S15.1.3.4_A3.1_T1.js

const uriReserved: string[] = ["%3B", "%2F", "%3F", "%3A", "%40", "%26", "%3D", "%2B", "%24", "%2C"];
const uriReserved_: string[] = [";", "/", "?", ":", "@", "&", "=", "+", "$", ","];

function main(): void {
  for (let indexC = 0; indexC < 10; indexC++) {
    const str = uriReserved_[indexC];
    assertSameValue(encodeURIComponent(str), uriReserved[indexC], "#" + String(indexC + 1) + ": unescapedURIComponentSet not containing" + str);
  }
}
