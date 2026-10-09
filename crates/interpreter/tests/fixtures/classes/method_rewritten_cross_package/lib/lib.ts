/** Greets, through a method an importer may rewrite. */
export class Greeter {
  /** The greeting. */
  greet(): string { return "hello"; }
  /** A method nothing rewrites. */
  fixed(): string { return "fixed"; }
}
