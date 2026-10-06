function main(): void {
  const source = "^[a-z]+$";
  const implicit = new RegExp(source);
  const explicit = new RegExp(source, "");
  assert(implicit.source === source, "source is preserved");
  assert(implicit.flags === "", "omitted flags default to empty");
  assert(!implicit.global && !implicit.ignoreCase && !implicit.multiline,
    "omitted flags leave matching options disabled");
  assert(!implicit.dotAll && !implicit.unicode && !implicit.sticky,
    "omitted flags leave other options disabled");
  assert(implicit.test("abc") === explicit.test("abc"), "matching agrees");
  assert(!implicit.test("ABC"), "matching remains case sensitive");
  assert(implicit.test("abc"), "repeated tests are stateless without global flags");
  assert(new RegExp(source, "i").test("ABC"), "explicit flags still work");
  assert(new RegExp("").test("anything"), "empty pattern accepts omitted flags");
  assert(new RegExp("abc").exec("abc") !== null, "exec accepts omitted flags");

  let rejected = false;
  try {
    new RegExp("[");
  } catch (error) {
    rejected = error instanceof SyntaxError;
  }
  assert(rejected, "invalid patterns still throw SyntaxError");
}
