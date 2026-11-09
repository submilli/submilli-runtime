// SUB-2102, SUB-2189: null and objects remain outside Number's accepted inputs.
// Their JavaScript conversions are an established Submilli deviation.
// expect-error-count: 2
// expect-error: got `null`
// expect-error: got `number[]`
function main(): void {
  console.log(Number(null), Number([1]));
}
