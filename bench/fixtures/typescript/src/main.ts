import { Store } from "./store";
import { Cache } from "./cache";
import { helper, checksum as cs, format } from "./util";
import * as other from "./other";

export function main(): void { // @def main
  const store = new Store();
  store.save("a");
  const cache = Cache.build(4);
  cache.save("b");
  const total = store.size() + cache.size();
  helper();
  other.helper();
  console.log(format(total), cs("z"));
  run(store);
}

function run(store: Store): number { // @def run
  store.save("again");
  const inner = (x: string) => cs(x);
  return inner("q");
}

const handler = async () => { // @def handler
  const r = await fetch("/x");
  return r.json();
};

main();
