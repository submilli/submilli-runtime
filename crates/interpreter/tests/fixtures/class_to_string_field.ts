// A class field named `toString` that holds a function is what `String(x)`
// and template interpolation call, as in JavaScript.
class Labelled {
  v: number = 1;
  toString: () => string = () => "labelled";
}

class Child extends Labelled {
  w: number = 2;
}

class Reading {
  v: number = 3;
  toString: () => string = () => "v=" + String(this.v);
}

function main(): void {
  const labelled = new Labelled();
  assert(String(labelled) === "labelled", "String of a toString field");
  assert(`${labelled}` === "labelled", "interpolating a toString field");
  assert(String(new Child()) === "labelled", "an inherited toString field");
  assert([new Child(), new Labelled()].join("|") === "labelled|labelled", "toString fields through join");
  assert(JSON.stringify(new Child()) === '{"v":1,"w":2}', "a toString field is not serialized");
  const items: (Labelled | null)[] = [new Labelled(), null];
  assert(items.join("-") === "labelled-", "joining a nullable instance");
  labelled.toString = () => "changed";
  assert(String(labelled) === "changed", "a reassigned toString field");
  assert(String(new Reading()) === "v=3", "a toString field reading this");
}
