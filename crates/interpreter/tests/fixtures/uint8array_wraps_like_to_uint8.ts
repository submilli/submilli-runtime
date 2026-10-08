// A number stored in a `Uint8Array` goes through JavaScript's `ToUint8`:
// truncate toward zero, then reduce modulo 256. Negatives wrap (`-1` is 255)
// rather than clamping to 0; `NaN` and `±Infinity` become 0.
function bytes(a: Uint8Array): string {
  const out: number[] = [];
  for (let i = 0; i < a.length; i++) {
    out.push(a[i]);
  }
  return out.join(",");
}

function main(): void {
  const built = new Uint8Array([-1, -128, -255, 256, 1.7, -1.7, 300, -0.5, 0 / 0, 1 / 0, -1 / 0, 2 ** 60 + 2 ** 9]);
  assert(bytes(built) === "255,128,1,0,1,255,44,0,0,0,0,0", "the constructor wraps");

  const written = new Uint8Array(4);
  written[0] = -1;
  written[1] = 257.9;
  written[2] = -257;
  written[3] = 0 / 0;
  assert(bytes(written) === "255,1,255,0", "an index write wraps");

  written[1] -= 2;
  assert(written[1] === 255, "a compound write wraps");

  written.fill(-2);
  assert(bytes(written) === "254,254,254,254", "fill wraps");
  assert(Uint8Array.of(-3, 511).join(",") === "253,255", "of wraps");
  assert(new Uint8Array([1, 2]).map((b: number) => b - 3).join(",") === "254,255", "map wraps");
}
