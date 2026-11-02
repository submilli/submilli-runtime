import fs from "submilli:fs";
class Options {
  reads: number = 0;
  get recursive(): boolean { this.reads += 1; return true; }
}
class Refuse {
  get recursive(): boolean { throw new Error("getter refused"); }
}
function main(): void {
  const options = new Options();
  fs.mkdir("/nested/dir", options);
  assert(options.reads === 1 && fs.exists("/nested/dir"));
  fs.writeText("/nested/dir/file", "x");
  const entries = Array.from(fs.list("/nested", options));
  assert(options.reads === 2 && entries.length === 2);
  fs.remove("/nested", options);
  assert(options.reads === 3 && !fs.exists("/nested"));
  let thrown = false;
  try { fs.mkdir("/refused", new Refuse()); }
  catch (e) { thrown = e.message === "getter refused"; }
  assert(thrown && !fs.exists("/refused"));
  fs.mkdir("/default", {});
  fs.remove("/default");
}
