export class Base {
  a: string = "base-a";
  v: string = "base-v";

  read(): string {
    return this.v;
  }
}

// Producer-side shadowing: the exported layout the consumer reconstructs
// already has `v` deduped against `Base`'s.
export class Shadowed extends Base {
  v: string = "shadowed-v";
  z: string = "shadowed-z";
}

// A second level, so the consumer's prefix walk crosses two shadowing links.
export class Deeper extends Shadowed {
  v: string = "deeper-v";
}
