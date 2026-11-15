function main(): void {
  const xs: number[] = JSON.parse("[1,2,3]");
  assert(xs.join(",") === "1,2,3", "annotated const");
  let user: { name: string } = (JSON.parse('{"name":"Ada"}'));
  assert(user.name === "Ada", "parenthesized annotated let");
  const bracket: number = JSON["parse"]("4");
  assert(bracket === 4, "string key spelling");
  const raw: unknown = JSON.parse("null");
  assert(raw === null, "unknown stays unknown");
  let implicit = "";
  let explicit = "";
  try {
    const bad: number[] = JSON.parse('["a"]');
  } catch (e: Error) {
    assert(e instanceof TypeError, "implicit mismatch throws TypeError");
    implicit = e.message;
  }
  try {
    const bad = JSON.parse('["a"]') as number[];
  } catch (e: Error) {
    explicit = e.message;
  }
  assert(implicit.length > 0 && implicit === explicit, "same checked cast");
}
