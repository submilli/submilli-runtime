// `new` followed by `:` or `?`, or quoted, is an ordinary member name in an
// object type; only `new (` or `new <` starts a construct signature.
type Factory = { new: () => number };
type Quoted = { "new"(): number };
type Optional = { new?(): string };

const factory: Factory = { new: (): number => 42 };
const quoted: Quoted = { new: (): number => 7 };
const optional: Optional = {};

function main(): void {
  console.log(factory.new() + quoted.new());
  console.log((optional.new ?? null) === null);
}
