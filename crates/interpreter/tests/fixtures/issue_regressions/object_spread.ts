let order: string = "";
function value(): number { order = order + "v"; return 1; }
function source(): { a?: string } { order = order + "s"; return {}; }
function pick(): boolean { return true; }
function main(): void {
  const absent: { a?: string } = {};
  const kept = { a: 123, ...absent };
  assert(kept.a === 123, "absent optional preserves earlier value");
  const present: { a?: string } = { a: "text" };
  const replaced = { a: 123, ...present };
  assert(replaced.a === "text", "present optional replaces earlier value");
  const conditional = { a: 123, ...(pick() ? { a: "text" } : {}) };
  assert(conditional.a === "text", "conditional spread preserves fields");
  const wide = { a: "text", extra: 9 };
  const narrow: {} = wide;
  const hidden = { a: 123, ...narrow };
  assert(Object.hasOwn(hidden, "extra"), "copy fields hidden by annotation");
  assert(JSON.stringify(hidden) === '{"a":"text","extra":9}', "serialize actual spread values");
  assert(JSON.stringify({ nested: hidden }) === '{"nested":{"a":"text","extra":9}}', "nested dynamic fields");
  assert(JSON.stringify({ list: [hidden] }) === '{"list":[{"a":"text","extra":9}]}', "dynamic fields in arrays");
  order = "";
  const sequenced = { a: value(), ...source(), b: value() };
  assert(order === "vsv" && sequenced.a === 1 && sequenced.b === 1, "source order");
  order = "";
  const override: { a?: number } = { a: 2 };
  const overwritten = { a: value(), ...override };
  assert(order === "v" && overwritten.a === 2, "overwritten expression still runs");
}
