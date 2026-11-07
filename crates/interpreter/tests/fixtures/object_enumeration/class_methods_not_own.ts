// A class instance's methods live on its prototype in JavaScript, so they are
// not own properties: `Object.keys`, `values`, `entries`, `hasOwn` and a
// record check skip them. `in` still finds them. A field holding a function
// is an own property. A spread or `Object.assign` copy doesn't list them
// either, and a method never replaces a field of the same name.

class Base {
    b: number = 0;
    hi(): number {
        return this.secret();
    }
    private secret(): number {
        return 1;
    }
}

class Derived extends Base {
    x: number = 1;
    twice: (n: number) => number = (n) => n * 2;
    get doubled(): number {
        return this.x * 2;
    }
    move(): void {
        this.x += 1;
    }
}

class Point {
    b: number = 0;
    x: number = 1;
    move(): void {
        this.x += 1;
    }
}

class Counter {
    last: number = 0;
    set move(step: number) {
        this.last = step;
    }
}

class Named {
    name(): string {
        return "named";
    }
}

class Numbered {
    name(): number {
        return 1;
    }
}

type XY = { b: number; x: number };
type HasMove = { move: number | (() => void) };

interface Mover {
    x: number;
    move(): void;
}

function main(): void {
    const d = new Derived();
    assert(Object.keys(d).sort().join() === "b,twice,x");
    assert(Object.values(d).length === 3);
    assert(Object.entries(d).map(([k]) => k).sort().join() === "b,twice,x");
    assert(!Object.hasOwn(d, "move") && !Object.hasOwn(d, "hi"));
    assert(!Object.hasOwn(d, "secret"));
    assert(Object.hasOwn(d, "x") && Object.hasOwn(d, "twice"));
    assert("move" in d);
    const record = d as unknown as Record<string, number | ((n: number) => number)>;
    assert(Object.values(record).length === 3);

    // Methods still dispatch through the instance.
    const mover: Mover = d;
    mover.move();
    assert(d.x === 2 && d.hi() === 1 && d.doubled === 4 && d.twice(3) === 6);

    // Copies of an instance seen through a data-only type list its data.
    const xy: XY = new Point();
    const spread = { ...xy };
    assert(Object.values(spread).join() === "0,1" && !Object.hasOwn(spread, "move"));
    assert(Object.values(Object.assign({ z: 7 }, xy)).join() === "7,0,1");
    const overField: HasMove = { move: 5, ...xy };
    assert(overField.move === 5);
    assert(Object.assign({ move: 5 }, xy).move === 5);
    const overMethod = { ...xy, move: 6 };
    assert(Object.hasOwn(overMethod, "move") && overMethod.move === 6);

    // A setter of that name isn't called, and an array target is untouched.
    assert(Object.assign(new Counter(), new Point()).last === 0);
    assert(Object.assign([1], new Named()).length === 1);
    // A later method replaces an earlier one, as the result's type says.
    const numbered: { name(): number } = new Numbered();
    const named: { name(): string } = new Named();
    const both = { ...numbered, ...named };
    assert(both.name() === "named");

    // An object literal's methods are its own properties.
    const literal = { f: () => 1, m(): number { return 2; }, x: 1 };
    assert(Object.keys(literal).join() === "f,m,x");
}
