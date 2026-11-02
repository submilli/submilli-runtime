function copy<T>(xs: T[]): T[] { return xs; }
function main(): void {
  assert(/x/.exec("y") === null && "y".match(/x/) === null);
  const re = /(?<letter>a)(b)?/g;
  const hit = re.exec("a");
  if (hit === null) { throw new Error("missing match"); }
  assert(Array.isArray(hit));
  assert(hit === hit && [hit].includes(hit));
  assert(hit.length === 3 && hit[0] === "a" && hit[1] === "a" && hit[2] === undefined);
  assert(hit.index === 0 && hit.input === "a");
  const groups = hit.groups;
  assert(groups !== undefined && groups["letter"] === "a");
  assert([...hit].length === 3);
  assert(Array.from(hit).length === 3 && copy(hit).length === 3);
  assert(hit["0"] === "a");
  assert(Object.hasOwn(hit, "groups"));
  assert(Object.keys(hit).join(",") === "0,1,2,index,input,groups");
  assert(hit.join("-") === "a-a-");
  const plain = /x/.exec("x");
  assert(plain !== null && plain.groups === undefined);
  const global = "aba".match(re);
  assert(global !== null && global.length === 2 && global[0] === "ab" && global[1] === "a");
  assert(global !== null && global.index === undefined && global.input === undefined && global.groups === undefined);
  const erased: unknown = global;
  const restored = erased as RegExpMatchArray;
  assert(restored.length === 2);
  assert(re.lastIndex === 0);
  re.lastIndex = 1.5;
  assert(re.lastIndex === 1.5);
  const iter = "aba".matchAll(re);
  re.lastIndex = 0;
  const first = iter.next();
  assert(!first.done && first.value.index === 2 && first.value[0] === "a");
  assert(iter.next().done === true && iter.next().done === true);
  assert(re.lastIndex === 0);
  let threw = false;
  try { "aba".matchAll(/a/); } catch (e) { threw = e instanceof TypeError; }
  assert(threw);
  const fromString = Array.from("aba".matchAll("a"));
  assert(fromString.length === 2 && fromString[1].index === 2);
}
