// test262: test/built-ins/Uint8Array/fromBase64/whitespace.js
// .buffer.byteLength assertions dropped (no ArrayBuffer backing).
// expect-fail: fromBase64 rejects ASCII whitespace in the input; the standard ignores space/tab/LF/FF/CR between base64 characters

function main(): void {
  const whitespace: string[] = ["Z g==", "Z\tg==", "Z\ng==", "Z\fg==", "Z\rg=="];
  const kinds: string[] = ["space", "tab", "LF", "FF", "CR"];
  for (let i = 0; i < whitespace.length; i++) {
    const arr: Uint8Array = Uint8Array.fromBase64(whitespace[i]);
    assertSameValue(arr.length, 1, `ascii whitespace: ${kinds[i]} (length)`);
    assertSameValue(arr[0], 102, `ascii whitespace: ${kinds[i]}`);
  }
}
