// An object literal of spreads checked against an expected type keeps its own
// type, as in tsc. A field a conditional spread may leave out stays optional,
// so a binding narrowed by the write still reads it as possibly absent.
interface Props {
  a?: number;
  b?: number;
}

function parse(flag: boolean): Props {
  return { ...(flag && { a: 1 }), ...(!flag && { b: 2 }) };
}

function main(): void {
  let props: Props = { ...(Math.random() > 2 && { a: 1 }) };
  assert(props.a === undefined, "a conditional spread that copied nothing");
  props = { ...{ a: 1 } };
  assert(props.a === 1, "a plain spread");
  const parsed = parse(false);
  assert(parsed.a === undefined && parsed.b === 2, "spreads in a return");
}
