export interface Profile {
  name: string;
  note?: string;
}

export function profile(): Profile {
  return { name: "package" };
}
