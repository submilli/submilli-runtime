// A member quoted as "new" or "@call" is an ordinary method, as in TypeScript:
// it declares no construct or call signature.
interface Sig {
  "new"(x: number): string;
  "@call"(): number;
}
class Impl implements Sig {
  "new"(x: number): string {
    return "n" + x;
  }
  "@call"(): number {
    return 7;
  }
}
function main(): void {
  const s: Sig = new Impl();
  assert(s.new(2) === "n2" && s["@call"]() === 7);
  const o: Sig = {
    "new"(x: number): string {
      return "o" + x;
    },
    "@call"(): number {
      return 8;
    },
  };
  assert(o.new(1) === "o1" && o["@call"]() === 8);
  assert(typeof o === "object");
}
