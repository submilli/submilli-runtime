// A function literal returning literals of one primitive on several paths
// returns their union, as tsc infers it, so a generic call's array result
// holds only those values. A lone literal still widens, and a `let` holding
// a generic call's result widens the literals the callback returned.
function make<T>(f: () => T): T {
  return f();
}

function makeAll<T>(f: () => T): T[] {
  return [f()];
}

function main(): void {
  const flag = Math.random() > 2;
  const pick = () => {
    if (flag) {
      return 1;
    }
    return 2;
  };
  const picked: 1 | 2 = pick();
  assert(picked === 2, "a closure's returns form their union");

  const all = makeAll(() => {
    if (flag) {
      return "a";
    }
    return "b";
  });
  const kept: ("a" | "b")[] = all;
  assert(kept[0] === "b", "an array result keeps the union");

  const counts = makeAll(() => 3);
  counts.push(4);
  assert(counts.length === 2, "a lone literal widens");

  let made = make(() => {
    if (flag) {
      return 1;
    }
    return 2;
  });
  made = 7;
  assert(made === 7, "a let widens the callback's literals");

  const maybe = () => {
    if (flag) {
      return 5;
    }
    return undefined;
  };
  const five: 5 | undefined = maybe();
  assert(five === undefined, "a literal beside undefined stays");
}
