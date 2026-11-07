// SUB-2132: the whole match is a string; captures can be undefined.
function fromMatch(text: string): string {
  const match = text.match(/(a)(b)?/);
  return match === null ? "" : match[0];
}

function main(): void {
  assert(fromMatch("a") === "a");
  assert(fromMatch("x") === "");
  const match = "a".match(/(a)(b)?/);
  if (match === null) throw new Error("missing match");
  const whole: string = match[0];
  const keyed: string = match["0"];
  const parenthesized: string = match[(0)];
  const negativeZero: string = match[-0];
  const positiveZero: string = match[+0];
  const [destructured, first, optional] = match;
  const required: string = destructured;
  assert(whole === "a" && keyed === whole && parenthesized === whole);
  assert(negativeZero === whole && positiveZero === whole);
  assert(required === whole && first === "a" && optional === undefined);
  const chained: number | undefined = "a".match(/a/)?.[0].length;
  assert(chained === 1);
  const signedChain: number | undefined = "a".match(/a/)?.[-0].length;
  assert(signedChain === 1);
  const absent = "x".match(/a/);
  const absentWhole: string | undefined = absent?.[0];
  assert(absentWhole === undefined);

  const exec = /(a)(b)?/.exec("a");
  if (exec === null) throw new Error("missing exec match");
  const execWhole: string = exec[0];
  const [execDestructured, execFirst, execOptional] = exec;
  const execRequired: string = execDestructured;
  assert(execWhole === "a" && execRequired === execWhole);
  assert(execFirst === "a" && execOptional === undefined);

  for (const item of "aa".matchAll(/(a)(b)?/g)) {
    const itemWhole: string = item[0];
    const [itemDestructured, itemFirst, itemOptional] = item;
    const itemRequired: string = itemDestructured;
    assert(itemWhole === "a" && itemRequired === itemWhole);
    assert(itemFirst === "a" && itemOptional === undefined);
  }
  const global = "aa".match(/a/g);
  if (global === null) throw new Error("missing global match");
  const globalWhole: string = global[0];
  assert(globalWhole === "a");
  const empty = /(?:)/.exec("");
  if (empty === null) throw new Error("missing empty match");
  const emptyWhole: string = empty[0];
  assert(emptyWhole === "");
}
