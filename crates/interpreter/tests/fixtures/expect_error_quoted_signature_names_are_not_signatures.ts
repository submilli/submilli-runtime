// A member quoted as "new" or "@call" is a method, not a signature, so the
// value can't be called or constructed, as in TypeScript (TS2349, TS2351).
// expect-error: cannot call value of type `Sig`
// expect-error: `new` expects a constructor; `Sig` declares no construct signature
// expect-error-count: 2
interface Sig {
  "new"(x: number): string;
  "@call"(): number;
}
function use(s: Sig): void {
  s();
  new s(2);
}
function main(): void {}
