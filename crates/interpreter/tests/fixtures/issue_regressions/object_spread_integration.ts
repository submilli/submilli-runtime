interface Result { a: number; b?: number; }
interface Named { label: string; }
type Link = { value: number; next?: Link };
function main(): void {
  const original = { a: 1 };
  const contextual: Result = { ...original };
  contextual.b = 4;
  assert(contextual.b === 4, "contextual interface optional slot is writable");
  const structural: { a: number; b?: number } = { ...original };
  structural.b = 5;
  assert(structural.b === 5, "contextual structural optional slot is writable");
  const absent: { a?: string | null } = {};
  const merged = { a: 123, ...absent };
  const required: number | string | null = merged.a;
  assert(required === 123, "optional nullable absence keeps earlier value and type");

  const wide = { tag: 1, payload: { label: 5 }, extra: 7 };
  const view: { tag: number } | { tag: number; payload: Named } = wide as { tag: number } | { tag: number; payload: Named };
  const checked = { payload: { label: "keep" }, ...view };
  assert(checked.payload.label === "keep", "incompatible interface field keeps fallback");
  assert(JSON.stringify(checked) === '{"extra":7,"payload":{"label":"keep"},"tag":1}', "extra runtime fields survive known-field checks");

  const hidden = { tag: 2, node: { value: "wrong" } };
  const recursive: { tag: number } | { tag: number; node: Link } = hidden as { tag: number } | { tag: number; node: Link };
  const copied = { node: { value: 9 }, ...recursive };
  assert(copied.node.value === 9, "recursive field check preserves fallback");
}
