// The object-identity vtable slots (toString / toJson / equals / hash) for
// $string and $Array are host-owned Rust Funcs under submilli:prelude_vtable
// (SUB-581). This drives each slot through the guest's call_ref dispatch: a
// miswired global or a wrong host slot traps or returns the wrong answer.

function main(): void {
  // --- string equals + hash: Set/Map membership routes through slots 2 and 3.
  const seen = new Set<string>();
  seen.add("héllo");
  seen.add("wörld");
  assert(seen.has("héllo"), "string hash+equals: member found");
  assert(!seen.has("nope"), "string hash+equals: non-member absent");
  assert(seen.size === 2, "string set has two distinct keys");

  const counts = new Map<string, number>();
  counts.set("a", 1);
  counts.set("a", 2);
  assert(counts.get("a") === 2, "string key dedupes via hash+equals");

  // --- string toString: template substitution invokes slot 0.
  const s = "abc";
  assert(`${s}!` === "abc!", "string toString via template");

  // --- string toJson: JSON.stringify invokes slot 1 (escaping path).
  assert(JSON.stringify("a\"\n\\b") === "\"a\\\"\\n\\\\b\"", "string toJson escapes");

  // --- array toString: join via each element's toString slot.
  assert([1, 2, 3].join(",") === "1,2,3", "array toString joins elements");
  assert(`${[1, 2, 3]}` === "1,2,3", "array toString via template");

  // --- array toJson: slot 1 over each element.
  assert(JSON.stringify([1, 2, 3]) === "[1,2,3]", "array toJson serializes");
  assert(JSON.stringify(["a", "b"]) === "[\"a\",\"b\"]", "array toJson of strings");

  // --- array structural equals: slot 2, element-wise via element slots.
  assert([1, 2, 3] === [1, 2, 3], "array equals: equal contents");
  assert(!([1, 2] === [1, 2, 3]), "array equals: length mismatch");
  assert(!([1, 2, 3] === [1, 9, 3]), "array equals: element mismatch");
}
