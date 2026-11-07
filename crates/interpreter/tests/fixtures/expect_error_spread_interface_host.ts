// expect-error: cannot spread host-backed interface `Response` into an object literal
import type { Response } from "submilli:http";

function copy(response: Response): void {
  const result = { ...response };
}

function main(): void {}
