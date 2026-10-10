// A line break ends a type before `[`, as in TypeScript: `[` on the next line
// starts an index signature (or a statement), not an array type.
type Scores = {
  total: number
  [name: string]: number;
};

interface Counts {
  first: string[]
  [key: string]: string[];
}

interface Lookup {
  size: number
  (key: string): number
}

type Methods = {
  names(): string[]
  [key: string]: () => string[];
};

let spread: {
  values: number[]
} = { values: [1, 2] };

type Id = number
(1 + 2);
type Ids = Id[]
[1, 2].forEach((n: number): void => {
  console.log(n);
});

function main(): void {
  const scores: Scores = { total: 3 };
  scores["extra"] = 4;
  const counts: Counts = { first: ["a"] };
  const id: Id = 7;
  const ids: Ids = [id];
  const methods: Methods = { names: (): string[] => ["a", "b"] };
  console.log(scores.total + (scores["extra"] ?? 0));
  console.log(counts.first.length, spread.values.length, ids[0], methods.names().length);
}
