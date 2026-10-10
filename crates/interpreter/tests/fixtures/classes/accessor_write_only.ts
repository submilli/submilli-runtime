// A write-only accessor (setter, no getter): writing works; the backing state is
// observed through a separate read-only accessor.
class Logger {
  private last: string = "";

  set message(m: string) {
    this.last = m;
  }

  get recorded(): string {
    return this.last;
  }
}

function main(): void {
  const log = new Logger();
  log.message = "hello";
  assert(log.recorded === "hello");
  log.message = "world";
  assert(log.recorded === "world");
}
