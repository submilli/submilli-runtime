// A write narrows a binding to the value's own type, unless the declaration
// has something `readonly` for it to keep (see
// expect_error_readonly_survives_narrowing.ts). A subclass instance keeps its
// class, and an array its own element type. A declaration with `readonly` in
// it narrows to the declared member that accepts the value, so the binding's
// fields stay readable after the write.

class Base {
  hello(): string {
    return "base";
  }
}

class Sub extends Base {
  hello(): string {
    return "sub";
  }
}

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

class Point {
  readonly x: number = 1;
  len(): number {
    return this.x;
  }
}

class Moving extends Point {
  speed: number = 2;
}

class TreeNode {
  parent: TreeNode | null = null;
  children: TreeNode[] = [];
  readonly value: number;
  constructor(value: number) {
    this.value = value;
  }
}

type Tree = { readonly val: number; kids: Tree[] };
type Left = { readonly a: number; b: string };
type Right = { readonly a: number; c: number };
type Both = { readonly a: number; b: string; c: number };
type Pair = { readonly id: number; xs: number[] };
type Loose = { readonly id: number; tag: number; v: number | string };
type Twin = { readonly a: number[]; n: number };
interface TwinFace {
  readonly a: number[];
  n: number;
}

class Pt {
  readonly x: number = 1;
  y: number = 2;
}

interface Guarded {
  readonly cb: () => number;
  m(): number;
}
interface Open {
  cb: () => number;
  m(): number;
}
class Impl implements Guarded {
  cb: () => number = (): number => 1;
  m(): number {
    return 2;
  }
}

interface Full {
  readonly id: number;
  label?: string;
  describe(): string;
}
interface Slim {
  readonly id: number;
  describe(): string;
}
class Item {
  readonly id: number = 7;
  describe(): string {
    return "item";
  }
}
type Point2 = { readonly a: number; b: number };
type Point3 = { readonly a: number; b: number };

class Square {
  side: number = 2;
  get area(): number {
    return this.side * this.side;
  }
}

interface HasArea {
  area: number;
}

interface HasX {
  x: number;
  readonly y: number;
}

function readonlyDeclarations(): void {
  let p: Point | null = null;
  p = new Moving();
  assert(p.len() + p.x === 2, "a subclass keeps the readonly its ancestor declares");

  let m: Map<string, readonly number[]> | null = null;
  m = new Map<string, number[]>();
  m.set("k", [1, 2]);
  assert(m.get("k")!.length === 2, "a generic instance narrows to the declared arguments");

  let t: Tree | null = null;
  t = { val: 3, kids: [] };
  assert(t.val + t.kids.length === 3, "a recursive type with readonly inside narrows");

  const both: Both = { a: 5, b: "b", c: 6 };
  let o2: Left | Right | null = null;
  o2 = both;
  assert(o2.a === 5, "a value with readonly of its own narrows to the accepting members");

  const frozen: { readonly id: number; readonly xs: number[] } = { id: 7, xs: [] };
  let pair: Pair | null = null;
  pair = frozen;
  assert(pair.id === 7, "a readonly the member drops still narrows away null");

  let hx: HasX | null = null;
  hx = new Pt();
  assert(hx.x + hx.y === 3, "a class instance narrows to the interface it is written to");

  const strict: { readonly id: number; readonly tag: number; v: number } = { id: 1, tag: 2, v: 3 };
  let loose: Loose | null = null;
  loose = strict;
  loose.v = "s";
  assert(loose.v === "s", "a write keeps the declared field types");

  let twin: Twin | TwinFace | readonly string[] = [];
  twin = { a: [1], n: 1 };
  twin.n = 5;
  assert(twin.n === 5, "members with the same readonly narrow to one of them");

  let guarded: Guarded | Open | null = null;
  guarded = new Impl();
  assert(guarded.m() === 2, "equally specific members narrow to the one keeping the other's readonly");

  let item: Full | Slim | null = null;
  item = new Item();
  assert(item.describe() === "item" && item.id === 7, "a member listing fewer fields stands for the others");

  let points: Point2[] | Point3[] | string = "s";
  points = [{ a: 1, b: 2 }];
  points.push({ a: 3, b: 4 });
  assert(points.length === 2 && points[1].b === 4, "array twins narrow to one of them");

  let shape: HasArea | null = null;
  shape = new Square();
  assert(shape.area === 4, "a getter-only property narrows to the interface");

  const one: { readonly a: number } = { a: 1 };
  let either: { a: number } | readonly string[] = [];
  either = one;
  assert(!Array.isArray(either) && either.a === 1, "an object narrows away a readonly array member");

  let node: TreeNode | null = null;
  node = new TreeNode(6);
  node.children.push(new TreeNode(7));
  assert(node.value + node.children.length === 7, "a recursive class narrows");
}

function main(): void {
  readonlyDeclarations();

  let b: Base | Sub | null = null;
  b = new Sub();
  assert(b.hello() === "sub", "a union with a subclass member narrows to the value");

  let a: Animal | null = null;
  a = new Dog();
  let sawDog = false;
  if (a instanceof Dog) {
    sawDog = true;
  }
  assert(sawDog && a !== null && a.speak() === "woof", "narrowing survives an instanceof join");

  const pair: [number, number] = [1, 2];
  let list: number[] | [number, number] | null = null;
  list = pair;
  assert(list[0] + list.length === 3, "a tuple value keeps its tuple type");
}
