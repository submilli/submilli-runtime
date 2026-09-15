// expect-error: delete or rename one source file for module `wrap`
import { answer } from "./wrap";

export function main(): number {
  return answer();
}
