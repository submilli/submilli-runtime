// Each literal fits no member of its union, so only the assignment is
// reported and, as in tsc, no field is reported as unknown:
// 1. a tag that fits no member;
// 2. two tags that no one member fits together;
// 3. a spread, which may overwrite the tag, so no member is ruled out.
// expect-error: expected `Shape`, got `{ k: string; q: number }`
// expect-error: expected `ByNull`, got `{ a: string | null; c: number }`
// expect-error-count: 3
type Shape = { k: "x"; a: number } | { k: "y"; b: number };
type TwoTags = { k: "a"; m: 1; x: number } | { k: "b"; m: 2; y: number } | { k: "a"; m: 2; z: number };
type ByNull = { a: null; b: string } | { a: string; c: number };

function spreadOver(extra: { a?: string }): void {
  const spread: ByNull = { a: null, c: 4, ...extra };
}

function main(): void {
  const noneFit: Shape = { k: "z", q: 1 };
  const tagsConflict: TwoTags = { k: "b", m: 1, x: 1, y: 1 };
}
