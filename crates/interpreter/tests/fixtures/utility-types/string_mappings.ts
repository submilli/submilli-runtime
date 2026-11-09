type Event = "click" | "key_down";
function main(): void {
  const upper: Uppercase<Event> = "KEY_DOWN";
  const lower: Lowercase<"MiXeD"> = "mixed";
  const capital: Capitalize<Event> = "Click";
  const small: Uncapitalize<"Hello World"> = "hello World";
  const handler: `on${Capitalize<Event>}` = "onKey_down";
  const shout: Uppercase<string> = "LOUD";
  const tagged: Uppercase<`id-${string}`> = "ID-ABC";
  const keys: Record<Uppercase<Event>, number> = { CLICK: 1, KEY_DOWN: 2 };
  assert(upper === "KEY_DOWN" && lower === "mixed" && capital === "Click" && small === "hello World", "literal mappings");
  assert(handler === "onKey_down" && shout === "LOUD" && tagged === "ID-ABC" && keys.CLICK === 1, "templates and symbolic mappings");
}
