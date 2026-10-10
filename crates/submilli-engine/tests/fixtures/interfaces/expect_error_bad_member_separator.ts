// The terminator message names all three accepted forms — it replaced three
// separate `expected `;`` messages, so it has to be right for every member kind.
// expect-error: expected `;`, `,`, or `}` after interface member
interface I { x: number y: string }

function main(): void {
  console.log("unreachable");
}
