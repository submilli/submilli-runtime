type Options = { retries: number; verbose: boolean };
function main(): void {
  const options = JSON.parse("{\"retries\": 3}") as Partial<Options>;
  assert(options.retries === 3 && options.verbose === undefined, "a missing key passes the run-time check");
}
