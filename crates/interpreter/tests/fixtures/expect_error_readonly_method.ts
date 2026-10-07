// expect-error-count: 5
// expect-error: `readonly` can only modify a property or index signature
// `readonly` applies to properties and index signatures only (TypeScript's TS1024).
class K {
  readonly p: number = 1;
  readonly m(): number { return 1; }
  public readonly n(): number { return 1; }
  readonly get g(): number { return 1; }
}
type T = { readonly r(a: number): boolean; readonly f: () => void };
interface I { readonly s(): void; readonly q: number; }
function main(): void {}
