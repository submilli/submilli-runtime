function optionalView(value: [number, string | undefined]): [number, string?] { return value; }
function main(): void {
  const pair = optionalView([1, undefined]);
  assert(pair.length === 2, "required undefined position remains physically present");
  assert(pair[1] === undefined, "required union position satisfies optional element");
}
