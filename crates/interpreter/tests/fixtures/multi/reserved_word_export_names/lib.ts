import { static as value } from "./values";

export { static as package } from "./values";

/** The value exported under a reserved word. */
export function read(): number {
  return value;
}
