// expect-error: expected an array to spread or an iterable
function main(): void {
  const values = [...{ length: 2 }];
}
