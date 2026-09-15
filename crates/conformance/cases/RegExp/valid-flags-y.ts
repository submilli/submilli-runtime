// test262: test/built-ins/RegExp/valid-flags-y.js

function main(): void {
  const a = new RegExp("abc", "y");
  assert(a.sticky, "y alone");
  const b = new RegExp("abc", "gy");
  assert(b.global && b.sticky, "gy");
  const c = new RegExp("abc", "iy");
  assert(c.ignoreCase && c.sticky, "iy");
  const d = new RegExp("abc", "my");
  assert(d.multiline && d.sticky, "my");
  const e = new RegExp("abc", "uy");
  assert(e.unicode && e.sticky, "uy");
  const f = new RegExp("abc", "gimuy");
  assert(f.global && f.ignoreCase && f.multiline && f.unicode && f.sticky, "gimuy");
}
