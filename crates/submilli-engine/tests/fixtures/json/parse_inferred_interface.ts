interface TeamRef {
  id: string;
  name: string;
}

interface ListResponse {
  hasNextPage: boolean;
  teams: TeamRef[];
}

function expectMessage(thunk: () => void): string {
  try {
    thunk();
  } catch (e: Error) {
    return e.message;
  }
  assert(false, "expected JSON.parse to throw");
  return "";
}

function main(): void {
  // Object actual where an array was expected: JSON.parse returns unknown and
  // the `as` cast performs the runtime validation.
  const objectAsArray = expectMessage((): void => {
    const teams = JSON.parse(
      "{\"hasNextPage\":false,\"teams\":[{\"id\":\"t1\",\"name\":\"Eng\"}]}"
    ) as TeamRef[];
  });
  assert(objectAsArray.includes("type mismatch: expected TeamRef[]"), objectAsArray);
  assert(objectAsArray.includes("got object"), objectAsArray);

  // Array actual where an object was expected.
  const arrayAsObject = expectMessage((): void => {
    const response = JSON.parse(
      "[{\"id\":\"t1\",\"name\":\"Eng\"}]"
    ) as ListResponse;
  });
  assert(arrayAsObject.includes("type mismatch: expected ListResponse"), arrayAsObject);
  assert(arrayAsObject.includes("got object"), arrayAsObject);

  // Scalar actual.
  const scalarActual = expectMessage((): void => {
    const response = JSON.parse("42") as ListResponse;
  });
  assert(scalarActual.includes("type mismatch: expected ListResponse"), scalarActual);
  assert(scalarActual.includes("got number"), scalarActual);
}
