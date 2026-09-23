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

  let o: Left | Right | null = null;
  o = { a: 4, b: "b", c: 5 };
  assert(o.a === 4, "a value both members accept narrows to their union");

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
