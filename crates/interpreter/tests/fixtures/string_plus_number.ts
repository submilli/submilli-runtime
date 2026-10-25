// `+` concatenates a number onto a string the way JavaScript converts it, as
// `String(n)` does, on either side and in `+=`.
interface Entry { kind: string; path: string; size: number; }
function main(): void {
  const n: number = 1.5;
  assert("a" + 1 === "a1");
  assert(1 + "a" === "1a");
  assert("a" + n === "a1.5");
  assert("a" + NaN === "aNaN");
  assert("a" + -0 === "a0");
  assert("a" + 1e21 === "a1e+21");
  assert("a" + 0.1 * 3 === "a0.30000000000000004");
  assert(1 + 2 + "a" === "3a");
  assert("a" + 1 + 2 === "a12");
  const e: Entry = { kind: "file", path: "/notes", size: 42 };
  assert(e.kind + " " + e.path + " " + e.size + "\n" === "file /notes 42\n");
  let s = "n=";
  s += n;
  assert(s === "n=1.5");
  const sizes: string[] = [""];
  sizes[0] += e.size;
  assert(sizes[0] === "42");
  const box = { label: "#" };
  box.label += 7;
  assert(box.label === "#7");
  let mixed: string | number = 1;
  mixed += "a";
  assert(mixed === "1a");
}
