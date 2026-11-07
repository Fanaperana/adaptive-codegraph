export class Repo { // @def Repo
  save(x: number): number { // @def Repo::save
    return x;
  }
}

export class Queue { // @def Queue
  save(x: number): number { // @def Queue::save
    return x + 1;
  }
}

export class Cache { // @def Cache
  get(key: string): string { // @def Cache::get
    return key;
  }
}
