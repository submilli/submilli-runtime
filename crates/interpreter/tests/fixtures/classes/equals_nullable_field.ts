// Nullable class fields compare null-aware: both-null equal, one-null unequal.
class Tagged {
  id: number;
  tag: string | null;

  constructor(id: number, tag: string | null) {
    this.id = id;
    this.tag = tag;
  }
}

function main(): void {
  assert(new Tagged(1, null) === new Tagged(1, null));
  assert(new Tagged(1, null) !== new Tagged(1, "a"));
  assert(new Tagged(1, "a") !== new Tagged(1, null));
  assert(new Tagged(1, "a") === new Tagged(1, "a"));
  assert(new Tagged(1, "a") !== new Tagged(1, "b"));
}
