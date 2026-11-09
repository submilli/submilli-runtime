// SUB-2189: explicit boolean-to-number conversion also works through unions.
function convert(value: boolean | number): number {
  return Number(value);
}
function main(): void {
  assert(Number(true) === 1 && Number(false) === 0);
  assert(convert(true) === 1 && convert(false) === 0 && convert(7) === 7);
  const rows = [
    { allDownloaded: false, seconds: 30 },
    { allDownloaded: true, seconds: 10 },
    { allDownloaded: true, seconds: 20 },
    { allDownloaded: false, seconds: 40 },
  ];
  rows.sort((a, b) => Number(b.allDownloaded) - Number(a.allDownloaded) || b.seconds - a.seconds);
  assert(rows.map((row) => row.seconds).join(",") === "20,10,40,30");
  let calls = 0;
  function next(): boolean { calls++; return calls === 1; }
  assert(Number(next()) === 1 && Number(next()) === 0 && calls === 2);
  const literal: true = true;
  assert(Number(literal) === 1);
}
