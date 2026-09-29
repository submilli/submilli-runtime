// test262: test/built-ins/encodeURI/S15.1.3.3_A3.2_T1.js

const uriAlpha: string[] = ["A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z", "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x", "y", "z"];

function main(): void {
  for (let indexC = 0; indexC < uriAlpha.length; indexC++) {
    const str = uriAlpha[indexC];
    assertSameValue(encodeURI(str), str, "#" + String(indexC + 1) + ": unescapedURISet containing " + str);
  }
}
