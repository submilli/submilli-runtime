// A lone `;` is a valid loop body: the loop does all its work in its header.
let calls = 0;

function advance(): boolean {
  calls++;
  return calls < 5;
}

function main(): void {
  while (advance());
  assert(calls === 5, "while runs its condition until it fails");

  let i = 0;
  for (; i < 3; i++);
  assert(i === 3, "for runs its update clause");

  let after = 0;
  for (const x of [1, 2, 3]) ;
  after++;
  assert(after === 1, "the statement after an empty for-of is not its body");

  calls = 0;
  do ; while (advance());
  assert(calls === 5, "do-while runs its condition until it fails");

  let outer = 0;
  for (; outer < 2; outer++) for (let inner = 0; inner < 3; inner++);
  assert(outer === 2, "nested empty bodies");
}
