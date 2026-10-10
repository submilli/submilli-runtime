// `case null` doesn't cover an optional field's absent case, which JavaScript
// reads as `undefined`, so the code after the switch is reachable.
interface Tagged {
  tag?: "a";
}

function viaLiteral(o: { tag?: "a" }): string {
  switch (o.tag) {
    case "a":
      return "a";
    case null:
      return "null";
  }
  return "after";
}

function viaInterface(o: Tagged): string {
  switch (o.tag) {
    case "a":
      return "a";
    case null:
      return "null";
  }
  return "after";
}

function viaChain(o: { inner?: { tag: "a" } }): string {
  switch (o.inner?.tag) {
    case "a":
      return "a";
    case null:
      return "null";
  }
  return "after";
}

function main(): void {
  assert(viaLiteral({ tag: "a" }) === "a", "a present field matches its case");
  assert(viaInterface({ tag: "a" }) === "a", "through an interface");
  assert(viaChain({ inner: { tag: "a" } }) === "a", "through an optional chain");
}
