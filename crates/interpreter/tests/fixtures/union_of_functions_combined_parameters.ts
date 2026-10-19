// A union of function types combines parameter and result types as tsc's
// intersection and subtype reduction do: literal unions share their common
// members, callback parameters take a callback either member may call, and
// results that are subtypes of another member's drop out.
class Animal {
  speak(): string {
    return "...";
  }
}
class Dog extends Animal {
  speak(): string {
    return "woof";
  }
}

function literals(f: ((x: "a" | "b") => string) | ((x: "b" | "c") => string)): string {
  return f("b");
}

type Numbers = (cb: (n: number) => void) => number;
type Strings = (cb: (s: string) => void) => number;

function callbacks(f: Numbers | Strings, seen: string[]): number {
  return f((v: number | string) => {
    seen.push(typeof v);
  });
}

// The result is `Animal`, `Dog` being its subtype, so its methods can be called.
function results(f: ((a: Animal) => Animal) | ((d: Dog) => Dog)): string {
  return f(new Dog()).speak();
}

function main(): void {
  assert(literals((x: "a" | "b") => x + "!") === "b!");
  assert(literals((x: "b" | "c") => x + "?") === "b?");
  const seen: string[] = [];
  assert(callbacks((cb: (n: number) => void) => { cb(1); return 1; }, seen) === 1);
  assert(callbacks((cb: (s: string) => void) => { cb("s"); return 2; }, seen) === 2);
  assert(seen.join(",") === "number,string");
  assert(results((a: Animal) => a) === "woof");
  assert(results((d: Dog) => d) === "woof");
}
