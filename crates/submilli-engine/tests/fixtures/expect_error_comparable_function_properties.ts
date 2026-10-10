// expect-error-count: 3
// expect-error: expected `{ g: (arg0: Derived) => void; h: (arg0: Base) => void }`
// expect-error: expected `IP`, got `IQ`
// A function-typed property compares its parameters contravariantly, as in
// TypeScript, so properties whose parameters conflict can never be equal.
class Base { b: number = 1; }
class Derived extends Base { d: number = 1; }
function f5(p: { g: (x: Derived) => void; h: (x: Base) => void },
            q: { g: (x: Base) => void; h: (x: Derived) => void }): boolean { return p === q; }
function f7(p: { g: (x: Derived) => Derived }, q: { g: (x: Base) => Base }): boolean { return p === q; }
interface IP { g: (x: Derived) => void; h: (x: Base) => void }
interface IQ { g: (x: Base) => void; h: (x: Derived) => void }
function i5(p: IP, q: IQ): boolean { return p === q; }
function main(): void {}
