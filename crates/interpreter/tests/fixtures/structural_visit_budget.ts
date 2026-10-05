import session from "submilli:session";

function main(): void {
  // Cross the visit limit without spending most of the test on individual pushes.
  let left: number[] = [0];
  while (left.length < 100001) {
    left = left.concat(left);
  }
  left = left.slice(0, 100001);
  const right = left.slice();

  let comparisonLimited = false;
  try {
    Object.is(left, right);
  } catch (e: RangeError) {
    comparisonLimited = e.message.includes("structural visits");
  }
  assert(comparisonLimited, "wide walks also have a node budget");
  let directLimited = false;
  try {
    const same = left === right;
    assert(!same, "direct equality must reject the large walk");
  } catch (e: RangeError) {
    directLimited = e.message.includes("structural visits");
  }
  assert(directLimited, "direct guest slot calls share the outer budget");
  let jsonLimited = false;
  try {
    JSON.stringify(left);
  } catch (e: RangeError) {
    jsonLimited = e.message.includes("structural visits");
  }
  assert(jsonLimited, "direct array serialization shares the outer budget");
  let typedLimited = false;
  try {
    JSON.stringify({ values: left });
  } catch (e: RangeError) {
    typedLimited = e.message.includes("structural visits");
  }
  assert(typedLimited, "typed JSON preflight has its own visit budget");
  const dynamic: Record<string, unknown> = {};
  dynamic["values"] = left;
  let dynamicLimited = false;
  try {
    JSON.stringify({ dynamic: dynamic });
  } catch (e: RangeError) {
    dynamicLimited = e.message.includes("structural visits");
  }
  assert(dynamicLimited, "dynamic JSON fallback retains an outer visit budget");
  dynamic["values"] = [7];
  assert(JSON.stringify({ dynamic: dynamic }) === "{\"dynamic\":{\"values\":[7]}}",
    "dynamic serialization succeeds after a caught limit");
  assert(Object.is([1, 2], [1, 2]), "a caught limit restores the walk budget");
  assert(Object.is([3, 4], [3, 4]), "independent walks receive independent budgets");

  session.set("kept", 7);
  let sessionLimited = false;
  try {
    session.set("kept", left);
  } catch (e: TypeError) {
    sessionLimited = e.message.includes("structural visits");
  }
  assert(sessionLimited, "session validation bounds the number of visits");
  assert(session.get("kept") === 7, "rejection preserves the previous value");
  session.set("kept", [1, 2]);
  assert(Object.is(session.get("kept"), [1, 2]), "a later small value can be stored");
}
