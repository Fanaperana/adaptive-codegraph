export interface Listener { // @def Listener
  handle(event: string): void; // @def Listener::handle
}

export class EventBus { // @def EventBus
  private listeners: Listener[] = [];

  subscribe(l: Listener): void { // @def EventBus::subscribe
    this.listeners.push(l);
  }

  publish(event: string): void { // @def EventBus::publish
    for (const l of this.listeners) {
      l.handle(event);
    }
  }
}
