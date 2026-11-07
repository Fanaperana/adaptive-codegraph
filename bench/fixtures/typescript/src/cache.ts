export class Cache { // @def Cache
  private data = new Map<string, number>();

  constructor(private cap: number) {} // @def Cache::constructor

  save(key: string): void { // @def Cache::save
    if (this.data.size >= this.cap) {
      this.evict();
    }
    this.data.set(key, 1);
  }

  size(): number { // @def Cache::size
    return this.data.size;
  }

  evict(): number { // @def Cache::evict
    const fresh = new Cache(this.cap);
    return fresh.size();
  }

  static build(cap: number): Cache { // @def Cache::build
    return new Cache(cap);
  }
}
