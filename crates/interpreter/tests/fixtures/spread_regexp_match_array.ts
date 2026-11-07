function main(): void {
  const match = /a/.exec("a");
  assert(match !== null, "regex matched");
  if (match !== null) {
    const copy = { ...match };
    assert(
      JSON.stringify(copy) === '{"0":"a","index":0,"input":"a"}',
      "match array spread copies enumerable indices and metadata",
    );
  }

  const stringMatch = "a".match(/a/);
  assert(stringMatch !== null, "string matched");
  if (stringMatch !== null) {
    const copy = { ...stringMatch };
    assert(
      JSON.stringify(copy) === '{"0":"a","index":0,"input":"a"}',
      "string match array spread copies enumerable entries",
    );
  }
}
