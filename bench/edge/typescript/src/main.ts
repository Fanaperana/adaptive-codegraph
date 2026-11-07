import { Repo, Queue, Cache } from "./repo";
import { checksum } from "./index";
import { parse } from "./util";
import compute from "./util";

export class Service { // @def Service
  private repo: Repo;
  private cache?: Cache;

  constructor() { // @def Service::constructor
    this.repo = new Repo();
  }

  persist(x: number): number { // @def Service::persist
    return this.repo.save(x);
  }

  lookup(k: string): string | undefined { // @def Service::lookup
    return this.cache?.get(k);
  }
}

export function enqueue(): number { // @def enqueue
  const q = new Queue();
  return q.save(1);
}

export function viaReexport(): number { // @def viaReexport
  // checksum the payload first
  return checksum("payload");
}

export function arrowUser(): number { // @def arrowUser
  return parse("42");
}

export function defaultAlias(): number { // @def defaultAlias
  return compute(3);
}

export function describe(): string { // @def describe
  return "Repo.save() persists, checksum() validates";
}
