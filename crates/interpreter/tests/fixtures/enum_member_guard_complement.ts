enum Kind { Choice = 0, Alias = 0, Score = 1 }
function isChoice(kind: Kind): kind is Kind.Choice {
  return kind === Kind.Choice;
}
function classify(kind: Kind): Kind.Choice | Kind.Score {
  if (isChoice(kind)) {
    const choice: Kind.Choice = kind;
    return choice;
  }
  const score: Kind.Score = kind;
  return score;
}

enum Side { Left = "left", Right = "right" }
function isLeft(value: Side | null): value is Side.Left {
  return value === Side.Left;
}
function otherSide(value: Side | null): Side.Right | null {
  if (isLeft(value)) return null;
  return value;
}

function main(): void {
  assert(classify(Kind.Choice) === Kind.Choice);
  assert(classify(Kind.Alias) === Kind.Choice);
  assert(classify(Kind.Score) === Kind.Score);
  assert(otherSide(Side.Left) === null);
  assert(otherSide(Side.Right) === Side.Right);
  assert(otherSide(null) === null);
  console.log("ok");
}
