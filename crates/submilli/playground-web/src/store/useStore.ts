import { useSyncExternalStore } from "react";

import type { PageStore, Snapshot } from "./store";

/** The store's current snapshot; the component renders again whenever it changes. */
export function useStore(store: PageStore): Snapshot {
  return useSyncExternalStore(store.subscribe, store.getSnapshot);
}
