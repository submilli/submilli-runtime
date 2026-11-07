function format(value: string | undefined): string {
  return (JSON.stringify(value) ?? "").slice(0, 400);
}

function main(): void {
  assert(format(undefined) === "", "coalescing handles missing JSON");
  assert(format("author") === '"author"', "defined values retain their JSON");
  const values: string[] = [];
  assert("author=" + JSON.stringify(values.at(0)) === "author=undefined", "concatenation accepts undefined");
}
