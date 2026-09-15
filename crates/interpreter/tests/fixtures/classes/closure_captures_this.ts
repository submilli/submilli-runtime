// `this` inside a closure in a class member body. Arrow functions capture the
// receiver lexically, so `arr.map(x => this.f(x))` — the ordinary way to write
// this in TypeScript — has to work in every member body, including through
// nested closures and `super`.
class Base {
  greet(): string {
    return "base";
  }
}

class Counter extends Base {
  private step: number;
  private items: number[] = [1, 2, 3];
  private tag: string;

  constructor(step: number, tag: string) {
    super();
    this.step = step;
    // A closure in the constructor body, after `super(...)`.
    const decorate = (): string => tag + "/" + this.step.toString();
    this.tag = decorate();
  }

  scaled(): number[] {
    return this.items.map((x: number): number => x + this.step);
  }

  viaMethod(): number {
    // The closure calls back into an instance method.
    const f = (x: number): number => this.bump(x);
    return f(1);
  }

  bump(x: number): number {
    return x + this.step;
  }

  nested(): number {
    const outer = (): number => {
      const inner = (): number => this.step * 2;
      return inner();
    };
    return outer();
  }

  summed(): number {
    // `this` alongside a mutable captured local (boxed) in one closure.
    let sum = 0;
    this.items.forEach((x: number): void => {
      sum = sum + x + this.step;
    });
    return sum;
  }

  viaSuper(): string {
    const g = (): string => super.greet() + "-sub";
    return g();
  }

  get label(): string {
    const f = (): string => this.tag;
    return f();
  }

  set label(v: string) {
    const f = (s: string): string => s + this.tag;
    this.tag = f(v);
  }
}

function main(): void {
  const c = new Counter(10, "c");
  assert(c.scaled()[0] === 11, "this in a method-body closure");
  assert(c.viaMethod() === 11, "closure calling an instance method through this");
  assert(c.nested() === 20, "this through nested closures");
  assert(c.summed() === 36, "this alongside a boxed mutable capture");
  assert(c.viaSuper() === "base-sub", "super.method() inside a closure");
  assert(c.label === "c/10", "this in a getter closure, set from a ctor closure");
  c.label = "z";
  assert(c.label === "zc/10", "this in a setter closure");
}
