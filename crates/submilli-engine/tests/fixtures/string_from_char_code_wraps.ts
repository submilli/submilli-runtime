// `String.fromCharCode` converts each argument with JavaScript's `ToUint16`:
// truncate toward zero, then reduce modulo 65536, so negatives wrap.
function main(): void {
  const s = String.fromCharCode(-1, 65536 + 65, -65535, 1.9, 0 / 0, -0.5);
  const units: number[] = [];
  for (let i = 0; i < s.length; i++) {
    units.push(s.charCodeAt(i));
  }
  assert(units.join(",") === "65535,65,1,1,0,0", "each code wraps modulo 65536");
}
