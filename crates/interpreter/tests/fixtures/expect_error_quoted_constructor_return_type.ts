// A quoted "constructor" is the constructor, so it can't declare a return
// type, as in TypeScript (TS1093).
// expect-error: a constructor cannot declare a return type
class K {
  "constructor"(): number {
    return 7;
  }
}
function main(): void {}
